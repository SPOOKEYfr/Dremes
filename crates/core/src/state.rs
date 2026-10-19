//! Dremes state machine.
//!
//! This is the single source of truth for what the system believes is running.
//! The Engine mutates this state and the GUI renders it. Dremes never relies on
//! ad-hoc process checks alone — it drives an explicit state machine.

use serde::{Deserialize, Serialize};

/// Master engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EngineState {
    #[default]
    Off,
    Starting,
    Running,
    Stopping,
    Error,
}

impl EngineState {
    pub fn is_active(&self) -> bool {
        matches!(self, EngineState::Running | EngineState::Starting)
    }
    pub fn label(&self) -> &'static str {
        match self {
            EngineState::Off => "ENGINE OFF",
            EngineState::Starting => "ENGINE STARTING",
            EngineState::Running => "ENGINE RUNNING",
            EngineState::Stopping => "ENGINE STOPPING",
            EngineState::Error => "ENGINE ERROR",
        }
    }
}

/// Waydroid state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WaydroidState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
    /// The live probe could not determine a definitive state.
    Unknown,
}

/// Capture (video writer) state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptureState {
    #[default]
    Off,
    Starting,
    Running,
    Reconnecting,
    Stopping,
    Error,
}

/// Streaming state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StreamingState {
    #[default]
    Off,
    Starting,
    Active,
    Stopping,
    Error,
}

/// The active operating mode requested by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Mode {
    /// Nothing requested yet — idle.
    #[default]
    Idle,
    /// Gaming: Waydroid only, no capture.
    Gaming,
    /// Streaming: Waydroid + capture to OBS.
    Streaming,
}

impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Idle => "Idle",
            Mode::Gaming => "Gaming",
            Mode::Streaming => "Streaming",
        }
    }
}

/// Current capture metadata, if any.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureInfo {
    /// Current physical width, dynamically detected.
    pub width: u32,
    /// Current physical height, dynamically detected.
    pub height: u32,
    /// Pixel format (e.g. "YU12").
    pub format: String,
    /// Measured capture fps.
    pub fps: f32,
    /// Dropped-frame counter.
    pub dropped_frames: u32,
    /// The video device in use.
    pub device: String,
}

/// Overall system snapshot shared between Engine and GUI.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemState {
    pub engine: EngineState,
    pub waydroid: WaydroidState,
    pub capture: CaptureState,
    pub streaming: StreamingState,
    pub mode: Mode,

    /// Whether the engine is allowed / requested to be running (master switch).
    pub engine_enabled: bool,

    /// Current ADB endpoint host:port (empty when none).
    pub adb_endpoint: String,
    /// ADB connectivity status.
    pub adb_connected: bool,

    /// Capture details (filled while running).
    pub capture_info: CaptureInfo,

    /// Number of automatic recoveries performed this session.
    pub reconnect_count: u32,

    /// Human-readable status of the OBS-facing device.
    /// One of: "RUNNING", "NOT RUNNING", "NOT INSTALLED", "UNKNOWN".
    pub obs_status: String,

    /// Stream Boost enabled (safe capture/stream performance optimizations).
    pub stream_boost: bool,
    /// Short human-readable note describing the currently-active Stream Boost
    /// optimization (empty when off).
    pub stream_boost_note: String,

    /// Game Boost enabled (game/Waydroid-focused safe optimizations).
    pub game_boost: bool,
    /// Short human-readable note describing the currently-active Game Boost
    /// optimization (empty when off).
    pub game_boost_note: String,

    /// WiFi Boost enabled (wireless power-save disabled for game latency).
    pub wifi_boost: bool,
    /// Short human-readable note describing the currently-active WiFi Boost
    /// state (empty when off).
    pub wifi_boost_note: String,
    /// The wireless interface being boosted (empty when none detected).
    pub wifi_iface: String,

    /// Last SUPER-mode report (what was closed / kept). Empty when SUPER has
    /// not been used yet this session.
    pub super_report: String,

    /// Diagnostic detail captured during the last streaming attempt (for the
    /// Diagnostics page). Empty when there is nothing to report.
    pub v4l2_detail: String,
    /// The Waydroid container state from the last live probe.
    pub waydroid_container: String,
    /// The Waydroid session state from the last live probe.
    pub waydroid_session: String,

    /// The detected desktop/session environment (Wayland/X11, desktop family).
    pub session: crate::capture::SessionInfo,
    /// The active general-purpose capture session (idle by default). This is
    /// separate from `capture`/`CaptureInfo`, which describe the Waydroid video
    /// writer pipeline.
    pub capture_session: crate::capture::CaptureSessionInfo,
}

impl SystemState {
    /// Whether automatic recovery is currently supposed to be active.
    pub fn recovery_enabled(&self) -> bool {
        self.engine_enabled && self.streaming == StreamingState::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_off_by_default() {
        let s = SystemState::default();
        assert_eq!(s.engine, EngineState::Off);
        assert!(!s.engine_enabled);
        assert_eq!(s.mode, Mode::Idle);
    }

    #[test]
    fn engine_active_detection() {
        assert!(EngineState::Running.is_active());
        assert!(EngineState::Starting.is_active());
        assert!(!EngineState::Off.is_active());
    }

    #[test]
    fn recovery_only_when_engine_on_and_streaming() {
        let mut s = SystemState::default();
        assert!(!s.recovery_enabled());
        s.engine_enabled = true;
        s.streaming = StreamingState::Active;
        assert!(s.recovery_enabled());
        s.engine_enabled = false;
        assert!(!s.recovery_enabled());
    }

    #[test]
    fn all_states_default_off() {
        let s = SystemState::default();
        assert_eq!(s.waydroid, WaydroidState::Stopped);
        assert_eq!(s.capture, CaptureState::Off);
        assert_eq!(s.streaming, StreamingState::Off);
        assert_eq!(s.capture_session.state, crate::capture::CaptureRunState::Idle);
        assert!(s.capture_session.output_device.is_empty());
        assert_eq!(s.session.session_type, "unknown");
    }
}
