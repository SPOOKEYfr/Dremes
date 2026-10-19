//! Process ownership model.
//!
//! Dremes must only ever terminate processes it started (or processes it can
//! positively identify as Dremes-owned stale components). We track every child
//! process we spawn and tag it with a role. This model is the single authority
//! for deciding what is safe to stop.

use serde::{Deserialize, Serialize};

/// The role a Dremes-started process performs. Used to isolate process groups
/// so that, e.g., stopping capture never touches Waydroid or adb-server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProcessRole {
    /// scrcpy capture feeding /dev/video10.
    Capture,
    /// A V4L2 black-frame/fallback writer (if ever used), centrally owned.
    V4l2Writer,
    /// Waydroid session or container process.
    Waydroid,
    /// The adb-server process.
    AdbServer,
    /// FFmpeg reprocessing pipeline (optional).
    Ffmpeg,
    /// A general-purpose capture (scrcpy/ffmpeg/wf-recorder writing to a free
    /// loopback device). Never writes to the Waydroid capture device.
    GeneralCapture,
}

impl ProcessRole {
    /// A stable string identifier for the role.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessRole::Capture => "capture",
            ProcessRole::V4l2Writer => "v4l2-writer",
            ProcessRole::Waydroid => "waydroid",
            ProcessRole::AdbServer => "adb",
            ProcessRole::Ffmpeg => "ffmpeg",
            ProcessRole::GeneralCapture => "general-capture",
        }
    }
}

/// A process tracked as owned by Dremes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedProcess {
    /// OS process id.
    pub pid: u32,
    /// The role this process plays.
    pub role: ProcessRole,
    /// A marker so we can detect stale entries (bots that died without notice).
    pub alive_marker: bool,
    /// The original spawn time.
    pub spawned_at: u64,
}

impl OwnedProcess {
    pub fn new(pid: u32, role: ProcessRole, now: u64) -> Self {
        Self {
            pid,
            role,
            alive_marker: true,
            spawned_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_identity_is_stable() {
        assert_eq!(ProcessRole::Capture.as_str(), "capture");
        assert_eq!(ProcessRole::V4l2Writer.as_str(), "v4l2-writer");
        assert_ne!(ProcessRole::Capture, ProcessRole::Waydroid);
    }

    #[test]
    fn owned_process_records_pid_and_role() {
        let p = OwnedProcess::new(1234, ProcessRole::Capture, 5);
        assert_eq!(p.pid, 1234);
        assert_eq!(p.role, ProcessRole::Capture);
        assert!(p.alive_marker);
    }
}
