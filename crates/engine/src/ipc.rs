//! IPC between the GUI and the Engine.
//!
//! Protocol: newline-delimited JSON messages over a Unix domain socket
//! (Linux) or named pipe (Windows, future). The GUI sends Commands; the Engine
//! publishes State snapshots and Events. Only intent/state flows — no shell
//! commands from the GUI.

use serde::{Deserialize, Serialize};

use dremes_core::capture::{CaptureCatalog, CaptureDiagnosis, CaptureRequest, CaptureSessionInfo};
use dremes_core::state::SystemState;

/// An operational mode request/command sent by the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// Turn the Engine master switch on.
    EngineOn,
    /// Turn the Engine master switch off (proper shutdown sequence).
    EngineOff,
    /// Enter Gaming Mode (Waydroid only, no capture).
    GamingMode,
    /// Start streaming (Waydroid + capture to OBS).
    StartStreaming,
    /// Stop streaming (back to lightweight Gaming state).
    StopStreaming,
    /// Stop Waydroid entirely (no uninstall).
    StopWaydroid,
    /// Manually reconnect Waydroid to OBS (recovery operation).
    ReconnectToObs,
    /// Open OBS Studio (or focus it if already running). No-op if not installed.
    OpenObs,
    /// Gracefully close OBS Studio (explicit user request).
    CloseObs,
    /// Toggle Stream Boost on/off.
    StreamBoost,
    /// Toggle Game Boost on/off.
    GameBoost,
    /// Toggle WiFi Boost (wireless power-save) on/off.
    WifiBoost,
    /// SUPER mode: gracefully close background GUI apps.
    Super,
    /// Request a fresh state snapshot.
    GetState,
    /// Request a new resource snapshot.
    GetResources,
    /// Ping for liveness.
    Ping,

    // ---- General-purpose capture (independent of the Waydroid pipeline) ----
    /// Request the current capture catalog (session, backends, devices).
    GetCaptureCatalog,
    /// Start a general-purpose capture (e.g. Mobile / Monitor / Window / Camera).
    StartCapture(CaptureRequest),
    /// Stop the active general-purpose capture.
    StopCapture,
    /// Request the live status of the general-purpose capture session.
    GetCaptureStatus,
    /// Diagnose the capture stack (loopback, portals, OBS, stale state).
    CaptureDiagnose,
    /// Auto-repair what can be fixed without root, then return the diagnosis.
    CaptureFix,
}

/// A single structured log/event line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp: String,
    pub level: String,
    pub component: String,
    pub message: String,
}

/// Full snapshot published to the GUI after any state change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub status: SystemState,
    pub resources: dremes_core::platform::ResourceSnapshot,
    pub log_tail: Vec<LogEvent>,
}

/// The response the Engine returns after processing a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ack {
    pub ok: bool,
    pub error: Option<String>,
}

/// The full response envelope returned to the GUI over IPC.
///
/// Most commands return `Ack` (success/error). Queries (`GetState`,
/// `GetResources`) return a `Snapshot` body so the GUI can render live state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Plain acknowledgment of an intent command.
    Ack(Ack),
    /// The requested state/resources snapshot (from a query command).
    Snapshot(Snapshot),
    /// The capture catalog (from `GetCaptureCatalog`).
    Catalog(CaptureCatalog),
    /// The general-purpose capture session status (from `GetCaptureStatus`).
    CaptureStatus(CaptureSessionInfo),
    /// Capture-stack health report (from `CaptureDiagnose` / `CaptureFix`).
    CaptureDiagnosis(CaptureDiagnosis),
}

#[cfg(test)]
mod tests {
    use super::*;
    use dremes_core::capture::CaptureRequest;

    /// The GUI builds `{"StartCapture": {...}}` for payload commands. Lock the
    /// wire shape so a frontend change can never silently break the daemon.
    #[test]
    fn start_capture_wire_roundtrip() {
        let json = r#"{"StartCapture":{"kind":"Camera","source":"/dev/video0","output_device":"/dev/video11","width":1280,"height":720,"fps":30}}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match &cmd {
            Command::StartCapture(req) => {
                assert_eq!(req.kind, CaptureRequest::empty(dremes_core::capture::CaptureSourceKind::Camera).kind);
                assert_eq!(req.source, "/dev/video0");
                assert_eq!(req.output_device, "/dev/video11");
                assert_eq!(req.width, 1280);
                assert_eq!(req.fps, 30);
            }
            other => panic!("expected StartCapture, got {other:?}"),
        }
        // Re-serializing must produce the same JSON the frontend sent.
        assert_eq!(serde_json::to_value(&cmd).unwrap(), serde_json::from_str::<serde_json::Value>(json).unwrap());
    }

    #[test]
    fn unit_and_payload_commands_serialize_distinctly() {
        // Unit variant -> JSON string, matching the GUI's bare "command" form.
        assert_eq!(serde_json::to_value(Command::GetCaptureCatalog).unwrap(), serde_json::json!("GetCaptureCatalog"));
        // Tuple variant -> JSON object with the variant key (externally tagged).
        let inc = serde_json::to_value(Command::StartCapture(CaptureRequest::empty(dremes_core::capture::CaptureSourceKind::Monitor))).unwrap();
        assert!(inc.is_object());
        assert!(inc.get("StartCapture").is_some());
    }

    #[test]
    fn diagnose_fix_commands_roundtrip() {
        assert_eq!(serde_json::to_value(Command::CaptureDiagnose).unwrap(), serde_json::json!("CaptureDiagnose"));
        assert_eq!(serde_json::to_value(Command::CaptureFix).unwrap(), serde_json::json!("CaptureFix"));
        let back: Command = serde_json::from_str("\"CaptureDiagnose\"").unwrap();
        assert!(matches!(back, Command::CaptureDiagnose));
        // The response envelope for a diagnosis is externally tagged too.
        let resp = Response::CaptureDiagnosis(CaptureDiagnosis {
            healthy: true,
            issues: vec!["x".into()],
            notes: vec![String::new()],
            root_actions: vec![String::new()],
        });
        let v = serde_json::to_value(resp).unwrap();
        assert!(v.get("CaptureDiagnosis").unwrap().get("issues").is_some());
    }
}
