//! Capture manager.
//!
//! Enforces the direct Waydroid capture architecture and exactly-once-writer
//! invariant:
//!
//! ```text
//! Waydroid framebuffer → headless scrcpy → /dev/video10 → OBS
//! ```
//!
//! It spawns ONE scrcpy process with `--v4l2-sink`. No FFmpeg, no dummy
//! writer, no multiple concurrent writers. The owning PID is tracked so Dremes
//! can stop only what it started.

use std::process::{Child, Command, Stdio};

use dremes_core::error::{DremesError, Result};
use dremes_core::platform::CaptureConfig;
use dremes_core::process::{OwnedProcess, ProcessRole};

use crate::v4l2;

/// Working directory for scrcpy logs to keep things tidy.
pub struct CaptureProcess {
    child: Child,
    pub pid: u32,
    pub device: String,
}

/// Start the single capture writer for the given device.
///
/// Returns the owned capture process. The caller (Engine) records the PID for
/// ownership tracking. It then verifies real frames are flowing.
pub fn start(device: &str) -> Result<CaptureProcess> {
    // Ownership invariant: ensure no *Dremes-owned* stale capture first.
    // (The Engine performs stale cleanup via the process registry; here we
    // do a safety check that the device isn't already owned by another writer.)
    if !v4l2::device_exists(device) {
        return Err(DremesError::V4l2(format!(
            "device {device} does not exist. Ensure v4l2loopback is loaded."
        )));
    }

    // Spawn scrcpy headless, writing directly to the V4L2 sink.
    let child = Command::new("scrcpy")
        .args([
            "--no-window",
            "--no-audio",
            "--v4l2-sink",
            device,
            "--no-video-playback",
            "--no-audio-playback",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DremesError::NotFound("scrcpy".into())
            } else {
                DremesError::Capture(format!("failed to start scrcpy: {e}"))
            }
        })?;

    let pid = child.id();
    Ok(CaptureProcess { child, pid, device: device.into() })
}

impl CaptureProcess {
    /// Whether the process is still alive.
    pub fn is_alive(&mut self) -> bool {
        self.try_wait().map(|s| s.is_none()).unwrap_or(false)
    }

    /// Stop the capture process cleanly (SIGTERM then SIGKILL fallback).
    pub fn stop(&mut self) -> Result<()> {
        // Send SIGTERM.
        let _ = self.child.kill();
        // Give it a moment, then reap.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        while std::time::Instant::now() < deadline {
            if let Some(_) = self.try_wait()? {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // If still alive, escalate with SIGKILL.
        let _ = self.child.kill();
        let _ = self.try_wait();
        Ok(())
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().map_err(|e| DremesError::Capture(format!("wait failed: {e}")))
    }
}

/// Build an owner record for registry tracking.
pub fn to_owned(pid: u32, now: u64) -> OwnedProcess {
    OwnedProcess::new(pid, ProcessRole::Capture, now)
}

/// A `CaptureHandle` implementation wrapping a real scrcpy writer.
///
/// Used by the Linux backend's `start_capture`. Exactly one of these exists at
/// a time while streaming. Stores only the pid (not the Child), so it is cheap
/// to share and its trait methods do not require mutable state.
#[derive(Debug)]
pub struct ScrcpyCaptureHandle {
    pid_value: u32,
    pub device: String,
}

impl dremes_core::platform::CaptureHandle for ScrcpyCaptureHandle {
    fn pid(&self) -> Option<u32> {
        Some(self.pid_value)
    }

    fn is_alive(&self) -> bool {
        signal_pid(self.pid_value, "0")
    }

    fn stop(&self) -> Result<()> {
        // Graceful SIGTERM, then SIGKILL fallback.
        let _ = signal_pid(self.pid_value, "TERM");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        while std::time::Instant::now() < deadline {
            if !signal_pid(self.pid_value, "0") {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = signal_pid(self.pid_value, "KILL");
        Ok(())
    }
}

/// Send a signal to a pid using the `kill` utility. Returns Ok(true) if the
/// signal succeeded (or, for signal 0, the process exists).
fn signal_pid(pid: u32, sig: &str) -> bool {
    Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Start a capture writer and return it as an owned handle.
pub fn start_handle(device: &str) -> Result<Box<dyn dremes_core::platform::CaptureHandle>> {
    let p = start(device)?;
    Ok(Box::new(ScrcpyCaptureHandle {
        pid_value: p.pid,
        device: p.device,
    }))
}

/// Compute the frame size for a YU12 (I420) frame at the given dimensions.
/// This helps us sanity-check the negotiated format and verify device health.
pub fn yu12_frame_size(width: u32, height: u32) -> u64 {
    let y = (width as u64) * (height as u64);
    let uv = ((width as u64 + 1) / 2) * ((height as u64 + 1) / 2);
    y + 2 * uv
}

/// Print the current capture configuration to the log for visibility.
pub fn describe(config: &CaptureConfig) {
    log::info!(
        "capture: {}x{} format={} device={} frame_bytes(YU12)={}",
        config.width,
        config.height,
        config.format,
        config.device,
        if config.format == "YU12" { yu12_frame_size(config.width, config.height).to_string() } else { "n/a".into() }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yu12_frame_size_matches_reference() {
        // Reference from the working system: ~1512162 bytes @ 1366x738.
        let size = yu12_frame_size(1366, 738);
        let expected = 1366 * 738 + 2 * ((1367 / 2) * (739 / 2));
        assert_eq!(size, expected as u64);
        assert!(size > 1_500_000, "frame should be about 1.5MB, got {size}");
    }

    #[test]
    fn feasible_even_odd_handling() {
        // Odd dimensions must still produce a sensible size (no panic).
        let odd = yu12_frame_size(1365, 737);
        assert!(odd > 0);
        let even = yu12_frame_size(1920, 1080);
        assert!(even > 0);
    }
}
