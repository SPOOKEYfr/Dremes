//! OBS Studio helper.
//!
//! Detects whether OBS is installed, checks whether it is running, and opens /
//! focuses it. OBS is an *optional* consumer of `/dev/video10` — Dremes never
//! depends on it for the capture pipeline, but the user must be able to launch
//! it from the GUI.

use crate::command;

/// The desktop launcher command for OBS (resolved at runtime).
fn obs_binary() -> Option<&'static str> {
    if command::which("obs") {
        Some("obs")
    } else if command::which("obs-studio") {
        Some("obs-studio")
    } else {
        None
    }
}

/// Whether OBS Studio is installed on this system.
pub fn installed() -> bool {
    obs_binary().is_some()
}

/// Whether an OBS process is currently running. We match the OBS main binary
/// (e.g. `/usr/bin/obs`) — not its helper/plugin children.
pub fn running() -> bool {
    // Match a process whose executable name is `obs` or `obs-studio`.
    command::run_timeout("pgrep", &["-x", "obs"], 3)
        .map(|o| !o.trim().is_empty())
        .ok()
        .unwrap_or(false)
        || command::run_timeout("pgrep", &["-x", "obs-studio"], 3)
            .map(|o| !o.trim().is_empty())
            .ok()
            .unwrap_or(false)
}

/// Gracefully close OBS Studio (explicit user request only — the engine never
/// kills user processes on its own).
///
/// Returns:
/// - `Ok(true)`  => OBS was running and a graceful close was requested.
/// - `Ok(false)` => OBS was not running (nothing to close).
pub fn close() -> dremes_core::error::Result<bool> {
    if !running() {
        return Ok(false);
    }
    for name in ["obs", "obs-studio"] {
        let _ = command::run_timeout("pkill", &["-TERM", "-x", name], 5);
    }
    Ok(true)
}

/// Open OBS. If an instance is already running, focus/activate it (via
/// `wmctrl` if available, else a no-op since spawning a duplicate is wasteful).
///
/// Returns:
/// - `Ok(true)`  => OBS was opened (or an existing instance was activated).
/// - `Ok(false)` => OBS is not installed.
pub fn open() -> dremes_core::error::Result<bool> {
    let Some(bin) = obs_binary() else {
        return Ok(false);
    };

    if running() {
        // Already running: try to focus the existing window rather than
        // spawning a duplicate OBS instance.
        if command::which("wmctrl") {
            let _ = command::run_timeout("wmctrl", &["-a", "OBS"], 3);
        }
        return Ok(true);
    }

    // Launch detached so the engine isn't blocked on OBS's lifetime.
    let launched = std::process::Command::new(bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match launched {
        Ok(_) => Ok(true),
        Err(e) => Err(dremes_core::error::DremesError::Capture(format!(
            "Failed to launch OBS: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_or_not_is_boolean() {
        let _ = installed();
    }
}
