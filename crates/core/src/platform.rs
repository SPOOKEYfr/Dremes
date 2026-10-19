//! Platform abstraction.
//!
//! The Engine drives a `PlatformBackend` trait. The Linux implementation talks
//! to Waydroid / ADB / scrcpy / V4L2. A Windows implementation will exist
//! architecturally and gracefully report that Waydroid management is
//! unavailable, while the shared Dremes application still runs.

use crate::capture::{CaptureCatalog, CaptureRequest, SessionInfo};
use crate::error::{DremesError, Result};

/// A discovered capture/display configuration.
#[derive(Debug, Clone, Default)]
pub struct CaptureConfig {
    /// Physical width of the Waydroid display.
    pub width: u32,
    /// Physical height of the Waydroid display.
    pub height: u32,
    /// V4L2 pixel format (e.g. "YU12").
    pub format: String,
    /// The video device node (e.g. "/dev/video10").
    pub device: String,
    /// Target fps (0 = auto).
    pub fps: u32,
}

/// Result of a diagnostics check for one component.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiagnosticItem {
    pub key: String,
    pub label: String,
    /// "pass" | "warning" | "fail"
    pub status: &'static str,
    pub message: String,
}

/// The abstract operations the Engine needs from a platform.
///
/// This keeps UI/core free of Linux-specific shell logic.
pub trait PlatformBackend: Send + Sync {
    /// Human-readable name of the platform (e.g. "Linux" / "Windows").
    fn name(&self) -> &'static str;

    /// The one V4L2 device Dremes is allowed to write to.
    fn capture_device(&self) -> String;

    /// Whether the given V4L2 device node currently exists on the system.
    fn capture_device_exists(&self, device: &str) -> bool;

    // ---- Diagnostics ----
    fn run_diagnostics(&self) -> Vec<DiagnosticItem>;

    // ---- Waydroid ----
    fn waydroid_status(&self) -> Result<WaydroidStatus>;
    fn start_waydroid_session(&self) -> Result<()>;
    fn stop_waydroid_session(&self) -> Result<()>;

    // ---- ADB (dynamic discovery) ----
    /// Discover the live ADB endpoint for Waydroid and return host:port.
    fn discover_adb_endpoint(&self) -> Result<Option<String>>;
    /// Ensure the endpoint is connected and online.
    fn ensure_adb_connected(&self, endpoint: &str) -> Result<()>;

    // ---- OBS (optional desktop app) ----
    /// Detect whether OBS Studio is installed.
    fn obs_installed(&self) -> bool;
    /// Check whether an OBS process is currently running.
    fn obs_running(&self) -> bool;
    /// Open OBS, or focus/activate an already-running instance if practical.
    /// Returns Ok(true) if OBS was opened/activated, Ok(false) if not installed.
    fn open_obs(&self) -> Result<bool>;
    /// Gracefully close OBS Studio (explicit user request only). Returns
    /// Ok(true) if OBS was running and a close was requested.
    fn close_obs(&self) -> Result<bool> {
        Ok(false)
    }

    // ---- Capture ----
    /// Detect the physical Waydroid display resolution/format dynamically.
    fn detect_capture_config(&self) -> Result<CaptureConfig>;
    /// Validate a V4L2 device (then used to verify frames).
    fn validate_v4l2(&self, device: &str) -> Result<CaptureConfig>;
    /// Verify real frames are being produced on the device.
    fn verify_frames(&self, device: &str, seconds: u32) -> Result<u64>;
    /// Start the single capture writer and return a handle to it.
    ///
    /// The backend guarantees at-most-one writer: starting a second one should
    /// fail or be a no-op.
    fn start_capture(&self, device: &str) -> Result<Box<dyn CaptureHandle>>;

    // ---- General-purpose capture (independent of the Waydroid pipeline) ----
    /// Discover what can be captured right now: session info, available output
    /// devices (never the reserved Waydroid node), V4L2 sources and mobile
    /// devices. Safe to call while the engine is OFF — it is read-only.
    fn capture_catalog(&self) -> CaptureCatalog {
        CaptureCatalog {
            session: self.session_info(),
            ..Default::default()
        }
    }
    /// Detect the desktop/session environment from the ambient environment.
    fn session_info(&self) -> SessionInfo {
        SessionInfo::default()
    }
    /// Start a general-purpose capture from the given request. The backend
    /// resolves any empty `output_device`, never writes to the reserved
    /// Waydroid node, and owns at-most-one general capture at a time.
    ///
    /// Returns the resolved start details (method, device, connection) plus a
    /// live handle. Platforms that cannot do this return a clear "unsupported"
    /// error so the GUI shows an honest message.
    fn start_general_capture(&self, _request: &CaptureRequest) -> Result<GeneralCaptureStart> {
        Err(DremesError::msg(
            "General-purpose capture is not supported on this platform.",
        ))
    }
    /// Stop the active general-purpose capture lifecycle. Backends keep their
    /// own state; this is a convenience for the Engine to request a clean stop.
    fn stop_general_capture(&self) -> Result<()> {
        Ok(())
    }
    /// Capture-stack health report: v4l2loopback module/nodes, portal services,
    /// OBS presence and its PipeWire plugin. The Engine layers its own
    /// stale-session check on top. Must be cheap and read-only.
    fn capture_diagnosis(&self) -> crate::capture::CaptureDiagnosis {
        crate::capture::CaptureDiagnosis::healthy_default()
    }
    /// Screen-capture source types the desktop portal advertises (None when the
    /// host cannot report it). The Engine restarts a stale portal (0 types) so
    /// OBS's Screen Capture (PipeWire) source is not hidden.
    fn portal_source_types(&self) -> Option<u32> {
        None
    }

    // ---- Resource monitoring ----
    fn resource_snapshot(&self) -> ResourceSnapshot;

    /// Read-only hardware telemetry (temperatures, GPU, fan, battery). Cheap and
    /// safe to poll even while the Engine master switch is OFF, so the Resources
    /// page never needs the engine to be on to show temperatures.
    fn resource_telemetry(&self) -> ResourceSnapshot {
        ResourceSnapshot::default()
    }

    // ---- WiFi (optional wireless power-save toggling) ----
    /// Discover the active wireless interface and its power-save state.
    /// Returns `None` when no wireless interface is present.
    fn wifi_info(&self) -> Option<WifiInfo> {
        None
    }
    /// Apply a WiFi boost (power-save off) or restore it (power-save back on).
    /// Returns a short human-readable note describing what was applied.
    fn wifi_boost(&self, _on: bool) -> Result<String> {
        Err(DremesError::msg("WiFi boost is not supported on this platform."))
    }

    /// SUPER mode: gracefully close the user's background GUI applications
    /// (system apps and the Dremes stack are always kept). Returns a note
    /// describing what was closed and what was kept.
    fn super_close(&self) -> Result<String> {
        Err(DremesError::msg("SUPER mode is not supported on this platform."))
    }
}

/// Status snapshot for Waydroid.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WaydroidStatus {
    /// True when the Waydroid container (and session) are live.
    pub running: bool,
    /// True when the session reports running.
    pub session_running: bool,
    /// True when the container service/process is active.
    pub container_running: bool,
    /// "MAINLINE" (or similar) as reported by `waydroid status`.
    pub vendor_type: String,
    /// Primary session-line detail from `waydroid status`.
    pub detail: String,
    /// The container state string ("running"/"stopped"/"starting"/...).
    pub container_state: String,
    /// The session state string ("RUNNING"/"STOPPED"/...).
    pub session_state: String,
}

/// A lightweight resource snapshot.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceSnapshot {
    pub cpu_total: f32,
    pub ram_total_mb: f32,
    pub ram_used_mb: f32,
    pub engine_cpu: f32,
    pub engine_ram_mb: f32,
    pub waydroid_cpu: f32,
    pub waydroid_ram_mb: f32,
    pub capture_cpu: f32,
    pub capture_ram_mb: f32,
    pub adb_cpu: f32,
    pub adb_ram_mb: f32,

    // Optional hardware telemetry. `None` means the value is not exposed by
    // this machine / driver, and the GUI shows "—" instead of a fake number.
    /// Package/CPU temperature in °C.
    pub cpu_temp_c: Option<f32>,
    /// GPU utilization percent (0-100).
    pub gpu_util_pct: Option<f32>,
    /// GPU temperature in °C.
    pub gpu_temp_c: Option<f32>,
    /// Primary SSD/NVMe temperature in °C.
    pub ssd_temp_c: Option<f32>,
    /// Fan speed in RPM, when a hwmon fan input is exposed.
    pub fan_rpm: Option<f32>,
    /// Battery charge percent (0-100), when a battery is present.
    pub battery_pct: Option<f32>,
}

/// Discovered wireless information used by the WiFi Booster.
#[derive(Debug, Clone, Default)]
pub struct WifiInfo {
    /// Network interface name (e.g. "wlan0").
    pub iface: String,
    /// Whether the interface currently has power-save enabled.
    pub power_save_on: bool,
}

/// A handle to a running capture writer (e.g. a scrcpy process).
///
/// This is the cross-platform abstraction for "the process currently writing
/// to the capture device". The backend owns the concrete implementation; the
/// Engine only ever talks to this trait, so it can be mocked in tests.
pub trait CaptureHandle: Send + Sync {
    /// The OS pid of the writer, if known.
    fn pid(&self) -> Option<u32>;
    /// Whether the writer process is still alive.
    fn is_alive(&self) -> bool;
    /// Gracefully stop the writer (SIGTERM then SIGKILL fallback).
    fn stop(&self) -> Result<()>;
}

/// The resolved result of a successfully started general-purpose capture.
pub struct GeneralCaptureStart {
    /// Live handle to the spawned writer process.
    pub handle: Box<dyn crate::capture::GeneralCaptureHandle>,
    /// The resolved output device the writer writes to.
    pub output_device: String,
    /// The capture method in use (e.g. "scrcpy", "ffmpeg-x11grab").
    pub method: String,
    /// Connection detail (display string, ADB endpoint, video node...).
    pub connection: String,
    /// The concrete source reference used.
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
