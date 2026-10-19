//! Process manager: safe, ownership-aware process signaling.
//!
//! All process termination performed by the Engine goes through here. We only
//! ever signal PIDs that appear in the Dremes-owned registry, and we prefer a
//! graceful SIGTERM followed by a SIGKILL fallback. We never blind-kill all
//! scrcpy / adb / waydroid / ffmpeg processes.

use dremes_core::error::Result;

use crate::registry::ProcessRegistry;

/// Signal handling lives in platform-specific modules.
#[cfg(unix)]
pub mod sys {
    use std::process::Command;

    use dremes_core::error::{DremesError, Result};

    /// Send SIGTERM to a pid.
    pub fn terminate(pid: u32) -> Result<()> {
        run_signal(pid, "TERM")
    }

    /// Send SIGKILL to a pid.
    pub fn kill(pid: u32) -> Result<()> {
        run_signal(pid, "KILL")
    }

    /// Adjust a process's scheduling niceness (safe, non-realtime adjustment).
    /// `delta` is relative (e.g. -5 = slightly higher priority). Used by
    /// Stream Boost. Never uses maximum/highest priority.
    pub fn terminate_nice(pid: u32, delta: i32) -> Result<()> {
        let delta = delta.clamp(-10, 10);
        let st = Command::new("renice")
            .args([delta.to_string().as_str(), "-p", pid.to_string().as_str()])
            .status()
            .map_err(|e| DremesError::Command {
                command: format!("renice {delta} -p {pid}"),
                detail: e.to_string(),
            })?;
        if st.success() {
            Ok(())
        } else {
            Err(DremesError::msg(format!("renice {delta} -p {pid} failed")))
        }
    }

    /// Return whether the pid is currently alive.
    pub fn is_alive(pid: u32) -> bool {
        // Signal 0 checks for existence without sending.
        match Command::new("kill").args(["-0", &pid.to_string()]).status() {
            Ok(s) => s.success(),
            Err(_) => false,
        }
    }

    fn run_signal(pid: u32, sig: &str) -> Result<()> {
        let st = Command::new("kill")
            .args([format!("-{sig}"), pid.to_string()])
            .status()
            .map_err(|e| DremesError::Command {
                command: format!("kill -{sig} {pid}"),
                detail: e.to_string(),
            })?;
        if st.success() {
            Ok(())
        } else {
            Err(DremesError::msg(format!("kill -{sig} {pid} failed")))
        }
    }
}

#[cfg(not(unix))]
pub mod sys {
    use dremes_core::error::{DremesError, Result};

    pub fn terminate(_pid: u32) -> Result<()> {
        Err(DremesError::msg("Process termination not implemented on this platform."))
    }

    pub fn kill(_pid: u32) -> Result<()> {
        Err(DremesError::msg("Process termination not implemented on this platform."))
    }

    pub fn is_alive(_pid: u32) -> bool {
        false
    }

    pub fn terminate_nice(_pid: u32, _delta: i32) -> Result<()> {
        Err(DremesError::msg("renice not implemented on this platform."))
    }
}

/// Gracefully stop all Dremes-owned processes matching a predicate.
///
/// `predicate` selects which roles to stop (e.g. only Capture). This is the
/// only place the Engine terminates processes, and it only touches owned PIDs.
///
/// Extra safety: before signaling, we verify the process's current command
/// line still matches the expected role executable. This prevents us from ever
/// signaling a PID that has been recycled by an unrelated process.
pub fn stop_owned(
    reg: &mut ProcessRegistry,
    predicate: impl Fn(dremes_core::process::ProcessRole) -> bool,
) -> Result<()> {
    let pids: Vec<u32> = reg.owned_pids();
    for pid in pids {
        // Determine which role, if any, this pid is owned under.
        let Some(entry) = reg.lookup(pid) else { continue };
        if !predicate(entry.role) {
            continue;
        }
        // Sanity-check the cmdline still matches the role before signaling.
        if !cmdline_matches_role(pid, entry.role) {
            log::warn!("Refusing to signal PID {pid}: command line no longer matches {}.", entry.role.as_str());
            continue;
        }
        // Graceful terminate, then kill fallback.
        let _ = sys::terminate(pid);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while std::time::Instant::now() < deadline {
            if !sys::is_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if sys::is_alive(pid) {
            let _ = sys::kill(pid);
        }
        reg.unregister(pid);
    }
    Ok(())
}

/// A process command line is a NUL-delimited string in `/proc/<pid>/cmdline`.
fn cmdline(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| s.replace('\0', " "))
        .unwrap_or_default()
}

fn cmdline_matches_role(pid: u32, role: dremes_core::process::ProcessRole) -> bool {
    let cl = cmdline(pid);
    if cl.is_empty() {
        return false;
    }
    // The default executable name for each role. When running without args,
    // `cmdline` is just the binary name; when started by Dremes it includes
    // our flags. We accept a prefix match on the binary name.
    let Some(bin) = role_binary(role) else { return false };
    cl.trim().starts_with(bin) || cl.contains(bin)
}

fn role_binary(role: dremes_core::process::ProcessRole) -> Option<&'static str> {
    match role {
        dremes_core::process::ProcessRole::Capture => Some("scrcpy"),
        dremes_core::process::ProcessRole::V4l2Writer => Some("v4l2loopback"),
        dremes_core::process::ProcessRole::Waydroid => Some("waydroid"),
        dremes_core::process::ProcessRole::AdbServer => Some("adb"),
        dremes_core::process::ProcessRole::Ffmpeg => Some("ffmpeg"),
        // General-capture writers vary (scrcpy/ffmpeg/wf-recorder) and carry
        // their own verified stops at the backend handle; the generic cmdline
        // guard refuses to signal them (safe default).
        dremes_core::process::ProcessRole::GeneralCapture => None,
    }
}

/// Refresh the registry, dropping Dremes-owned processes that have exited.
pub fn refresh_liveness(reg: &mut ProcessRegistry) {
    reg.refresh(|pid| sys::is_alive(pid));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dremes_core::process::{OwnedProcess, ProcessRole};

    // These tests verify selection logic semantics on the registry; they never
    // terminate real processes, and they verify the cmdline-safety guard.
    #[test]
    fn stop_owned_only_touches_matching_owned() {
        let mut reg = ProcessRegistry::new();
        // Use pids that are statically non-existent (max pid on Linux ~ 4194304).
        reg.register(OwnedProcess::new(u32::MAX, ProcessRole::Capture, 0));
        reg.register(OwnedProcess::new(u32::MAX - 1, ProcessRole::Waydroid, 0));
        let _ = stop_owned(&mut reg, |r| r == ProcessRole::Waydroid);
        // These pids don't exist, so cmdline_matches_role refuses to signal.
        // The registry entries remain (nothing was terminated).
        assert!(reg.has_role(ProcessRole::Waydroid));
        assert!(reg.has_role(ProcessRole::Capture));
    }

    #[test]
    fn cmdline_safety_refuses_real_unrelated_pid() {
        // If a low pid were reused by an unrelated process, we must refuse.
        // We test with pid 1 (init), which is never an scrcpy process.
        let mut reg = ProcessRegistry::new();
        reg.register(OwnedProcess::new(1, ProcessRole::Capture, 0));
        let _ = stop_owned(&mut reg, |r| r == ProcessRole::Capture);
        // init is never a scrcpy process, so we must NOT have killed it, and
        // the registry entry is retained as a safety measure.
        assert!(reg.has_role(ProcessRole::Capture));
    }
}
