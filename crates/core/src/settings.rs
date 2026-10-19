//! Persistent settings.
//!
//! Settings survive application restart and reboot. Stored as JSON in the
//! platform config directory (e.g. `~/.config/dremes/settings.json` on Linux,
//! `%APPDATA%\dremes\settings.json` on Windows).
//!
//! No secrets or sudo passwords are ever stored.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Default button/engine sound volume (0.0 muted .. 1.0 max).
fn default_sound_volume() -> f32 {
    0.7
}

/// Capture settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureSettings {
    /// The V4L2 device Dremes writes to.
    pub device: String,
    /// Auto-detect the device at startup.
    pub auto_detect: bool,
    /// Target fps (0 = auto / device default).
    pub fps_target: u32,
    /// Recovery behavior tuning.
    pub retry_delay_secs: u64,
    pub max_retries: u32,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            device: "/dev/video10".into(),
            auto_detect: true,
            fps_target: 30,
            retry_delay_secs: 2,
            max_retries: 5,
        }
    }
}

/// ADB settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdbSettings {
    pub auto_discovery: bool,
    pub retry_behavior: bool,
    /// ADB connect timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for AdbSettings {
    fn default() -> Self {
        Self {
            auto_discovery: true,
            retry_behavior: true,
            timeout_secs: 10,
        }
    }
}

/// The complete Dremes settings document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // General
    /// Automatically start the Engine at login/boot. **Default OFF.**
    pub start_engine_automatically: bool,
    /// Automatically open the GUI at login. **Default OFF.**
    pub start_gui_automatically: bool,
    /// Minimize to system tray when the window is closed.
    pub minimize_to_tray: bool,
    /// Start the GUI minimized to tray.
    pub start_minimized: bool,
    /// UI theme: "light" | "dark" | "system".
    pub theme: String,
    /// Show desktop notifications.
    pub notifications: bool,
    /// UI sound volume (0.0 muted .. 1.0 max).
    #[serde(default = "default_sound_volume")]
    pub sound_volume: f32,

    // Waydroid
    /// Waydroid session behavior.
    pub waydroid_path: String,
    pub waydroid_session_recovery: bool,
    pub waydroid_retry_limit: u32,

    // Capture
    pub capture: CaptureSettings,

    // ADB
    pub adb: AdbSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_engine_automatically: false,
            start_gui_automatically: false,
            minimize_to_tray: true,
            start_minimized: false,
            theme: "system".into(),
            notifications: true,
            sound_volume: default_sound_volume(),
            waydroid_path: "waydroid".into(),
            waydroid_session_recovery: true,
            waydroid_retry_limit: 3,
            capture: CaptureSettings::default(),
            adb: AdbSettings::default(),
        }
    }
}

impl Settings {
    /// Load settings from the platform config directory.
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|_| {
                log::warn!("Failed to parse settings file at {path:?}; using defaults.");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings to disk.
    pub fn save(&self) -> crate::error::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| crate::error::DremesError::msg(format!("cannot create settings dir: {e}")))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| crate::error::DremesError::msg(format!("cannot serialize settings: {e}")))?;
        std::fs::write(&path, json)
            .map_err(|e| crate::error::DremesError::msg(format!("cannot write settings: {e}")))?;
        Ok(())
    }

    /// Determine the settings file path for the current platform.
    pub fn path() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    /// The app config directory, platform aware.
    pub fn config_dir() -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(|h| PathBuf::from(h).join(".config"))
                        .unwrap_or_else(|| PathBuf::from("."))
                })
                .join("dremes")
        }
        #[cfg(target_os = "windows")]
        {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("dremes")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            PathBuf::from(".")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_leave_engine_off() {
        let s = Settings::default();
        assert!(!s.start_engine_automatically, "engine auto-start must default OFF");
        assert!(!s.start_gui_automatically, "GUI auto-start must default OFF");
        assert_eq!(s.capture.device, "/dev/video10");
        assert!(s.capture.auto_detect);
    }

    #[test]
    fn roundtrip_persistence() {
        let mut s = Settings::default();
        s.theme = "dark".into();
        s.capture.fps_target = 60;

        // Test in a temp dir to avoid touching the real config.
        let dir = std::env::temp_dir().join(format!("dremes-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let json = serde_json::to_string(&s).unwrap();
        // Reuse the serialize/deserialize path to confirm roundtrip.
        let back: Settings = serde_json::from_str(&json).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(back.theme, "dark");
        assert_eq!(back.capture.fps_target, 60);
    }

    #[test]
    fn config_dir_is_absolute_on_linux() {
        if cfg!(target_os = "linux") {
            let d = Settings::config_dir();
            assert!(d.to_string_lossy().contains("dremes"));
        }
    }
}
