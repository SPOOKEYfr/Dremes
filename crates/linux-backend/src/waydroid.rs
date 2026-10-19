//! Waydroid manager.
//!
//! Detects Waydroid, reports live status, and starts/stops the session.
//!
//! Status is ALWAYS derived from a live probe — never from a stale cached
//! value. We probe both:
//! - the Waydroid **session** (`waydroid status`) and
//! - the Waydroid **container** (the `waydroid-container` service + lxc process)
//!
//! and reconcile them into a single definitive state so that running/stopped
//! is never guessed from a previous command having "succeeded". If Waydroid is
//! stopped outside Dremes, the next probe reflects it.

use dremes_core::error::{DremesError, Result};
use dremes_core::platform::WaydroidStatus;

use crate::command;

/// Max time we wait for a session/container transition during start/stop.
const TRANSITION_WAIT_MS: u64 = 45_000;

fn exec_status() -> Option<String> {
    command::run_timeout("waydroid", &["status"], 10).ok()
}

/// Recognized container states from `waydroid status` / systemctl.
fn container_class(s: &str) -> &'static str {
    let l = s.trim().to_lowercase();
    if l == "active" {
        "running"
    } else if l == "activating" || l.contains("starting") {
        "starting"
    } else if l == "inactive" || l == "failed" || l == "deactivating" {
        "stopped"
    } else if l.contains("running") {
        "running"
    } else if l.contains("stopping") {
        "stopping"
    } else if l.contains("stopped") || l.contains("stop") {
        "stopped"
    } else {
        "unknown"
    }
}

/// Recognized session states from the `Session:` line.
fn session_class(s: &str) -> &'static str {
    let l = s.trim().to_lowercase();
    if l.contains("running") {
        "running"
    } else if l.contains("stopping") {
        "stopping"
    } else if l.contains("starting") {
        "starting"
    } else if l.contains("stopped") || l.contains("stop") {
        "stopped"
    } else {
        "unknown"
    }
}

/// The `waydroid-container` service state, via both the user and system units
/// plus the lxc process. Returns (is_active, state_string).
fn container_service_state() -> (bool, String) {
    let user = command::run("systemctl", &["--user", "is-active", "waydroid-container"]);
    if let Ok(o) = user {
        let class = container_class(&o);
        return (class == "running" || class == "starting", o.trim().to_string());
    }
    let sys = command::run("systemctl", &["is-active", "waydroid-container"]);
    if let Ok(o) = sys {
        let class = container_class(&o);
        return (class == "running" || class == "starting", o.trim().to_string());
    }
    // Fall back to process presence: the container drives lxc.
    let lxc = command::run_timeout("pgrep", &["-f", "lxc-start.*waydroid"], 3)
        .map(|o| !o.trim().is_empty())
        .unwrap_or(false);
    (lxc, if lxc { "running" } else { "stopped" }.into())
}

/// Query Waydroid status and report a structured result based on a LIVE probe.
pub fn status() -> WaydroidStatus {
    let mut s = WaydroidStatus {
        running: false,
        session_running: false,
        container_running: false,
        vendor_type: String::new(),
        detail: String::new(),
        container_state: String::new(),
        session_state: String::new(),
    };

    // Session probe via `waydroid status` (authoritative for the user session).
    let mut session_class_observed = "unknown";
    if let Some(out) = exec_status() {
        let mut session_line: Option<String> = None;
        let mut vendor_line: Option<String> = None;
        let mut container_line: Option<String> = None;
        for line in out.lines() {
            let l = line.trim().to_lowercase();
            if l.starts_with("session:") {
                session_line = Some(line.trim().to_string());
            } else if l.starts_with("vendor type:") {
                vendor_line = Some(line.trim().to_string());
            } else if l.starts_with("container:") {
                container_line = Some(line.trim().to_string());
            }
        }
        if let Some(sess) = session_line {
            s.detail = sess.clone();
            s.session_state = session_class(&sess).to_string();
            session_class_observed = session_class(&sess);
            s.session_running = session_class_observed == "running";
        }
        if let Some(v) = vendor_line {
            s.vendor_type = v.clone();
        }
        // `waydroid status` also exposes the container line directly.
        if let Some(cont) = container_line {
            let c = container_class(&cont);
            s.container_state = c.to_string();
            s.container_running = c == "running";
        }
        // If the status command gave a definitive "stopped" for the session but
        // no container line, let the container service decide below.
    } else {
        s.detail = "waydroid not found on PATH or unresponsive".into();
        s.session_state = "unknown".into();
    }

    // Container probe (independent of the session probe).
    let (container_active, container_str) = container_service_state();
    if s.container_state.is_empty() {
        s.container_state = container_str.clone();
    }
    s.container_running = container_active || s.container_running;

    // Reconcile into `running`: both the container AND the session must be
    // live for a fully "running" Waydroid. A live container with a stopped
    // session is treated as not-yet-running (it may be starting/stopping).
    let session_up = s.session_running || session_class_observed == "starting";
    let container_up = s.container_running || s.container_state == "starting";
    s.running = s.session_running && s.container_running;

    // Surface a human-friendly combined summary in `detail` if not already set.
    if s.detail.is_empty() {
        s.detail = format!(
            "session={} container={}",
            s.session_state, s.container_state
        );
    }
    let _ = (session_up, container_up);
    s
}

fn command_ok(cmd: &str, args: &[&str]) -> bool {
    command::run(cmd, args).map(|o| o.trim() == "active").unwrap_or(false)
}

/// Start the Waydroid container and session, waiting until it is genuinely
/// ready. Never returns "success" before Waydroid reports Running.
pub fn start() -> Result<()> {
    // Start container via systemd (best-effort; the user session may already
    // own it). Requires the waydroid-container service.
    if !command_ok("systemctl", &["--user", "is-active", "waydroid-container"])
        && !command_ok("systemctl", &["is-active", "waydroid-container"])
    {
        log::info!("Starting waydroid-container service...");
        // Try user-level first, then system-level with a clear message.
        match command::run_timeout("systemctl", &["--user", "start", "waydroid-container"], 15) {
            Ok(_) => {}
            Err(_) => {
                match command::run_timeout("sudo", &["systemctl", "start", "waydroid-container"], 20) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("container start via sudo: {}", e);
                    }
                }
            }
        }
    }

    // Start the session (user-level). This is what makes the Android UI live.
    // `waydroid session start` is a long-lived process (it stays attached as a
    // daemon once ready), so it must be spawned DETACHED — never run under a
    // timeout, which would SIGTERM it and tear down the just-started session.
    log::info!("Starting Waydroid session...");
    let _ = command::spawn_detached("waydroid", &["session", "start"]);

    // Wait for a genuinely running state (both session and container live).
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(TRANSITION_WAIT_MS);
    while std::time::Instant::now() < deadline {
        let s = status();
        if s.running {
            log::info!("Waydroid is ready (session + container running)");
            return Ok(());
        }
        // If we've transitioned from starting into a stable error, bail early.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(DremesError::Waydroid {
        message: "Waydroid did not reach a Running state in time.".into(),
        suggestion: "Check that Waydroid is installed and initialized (waydroid init), and that the waydroid-container service can start.".into(),
    })
}

/// Stop the Waydroid session, waiting until it actually stops.
pub fn stop_session() -> Result<()> {
    let _ = command::run_timeout("waydroid", &["session", "stop"], 30);

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(TRANSITION_WAIT_MS);
    while std::time::Instant::now() < deadline {
        let s = status();
        if !s.running && !s.session_running {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Err(DremesError::Waydroid {
        message: "Waydroid session did not stop cleanly.".into(),
        suggestion: "You may stop the container manually if it remains stuck.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_returns_safely_when_absent() {
        // status() must never panic even if waydroid/adb are absent.
        let s = status();
        let _ = s.detail;
    }

    #[test]
    fn session_classifier_handles_state_strings() {
        assert_eq!(session_class("Session:\tRUNNING"), "running");
        assert_eq!(session_class("Session:\tSTOPPED"), "stopped");
        assert_eq!(session_class("Session:\tSTOPPING"), "stopping");
        assert_eq!(session_class("Session:\tSTARTING"), "starting");
        assert_eq!(session_class("Session:\tweird"), "unknown");
    }

    #[test]
    fn container_classifier_handles_state_strings() {
        assert_eq!(container_class("active"), "running");
        assert_eq!(container_class("inactive"), "stopped");
        assert_eq!(container_class("activating"), "starting");
    }
}
