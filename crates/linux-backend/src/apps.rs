//! SUPER mode: gracefully close the user's background GUI applications so
//! streaming and gaming get the CPU/GPU/RAM to themselves.
//!
//! Design rules (safety + truthfulness):
//! - Only processes owned by the same user AND connected to a graphical session
//!   (WAYLAND_DISPLAY / DISPLAY present in their environment) are candidates.
//! - Anything needed for Dremes to work is NEVER touched: Waydroid, scrcpy, adb,
//!   OBS, the Engine and the GUI itself, plus system services, compositors,
//!   credential/key agents and terminals (closing a shell mid-session is risky).
//! - Apps are closed with SIGTERM only (graceful). They are given two rounds to
//!   exit; never SIGKILLed, so unsaved work is not silently destroyed. Apps that
//!   refuse to exit are reported as "kept", never forced.
//! - The result note describes exactly what was closed and what was kept.

use dremes_core::error::{DremesError, Result};

use std::collections::BTreeSet;

/// Prefixes/names that are always protected from SUPER. Matched against the
/// lowercased process name and command line.
const PROTECTED: &[&str] = &[
    // System + session infra.
    "systemd",
    "gnome-shell",
    "gnome-session",
    "mutter",
    "dbus-daemon",
    "dbus-broker",
    "dbus",
    "xorg",
    "xwayland",
    "gsd-",
    "gjs",
    "gnome-keyring",
    "gnome-keyring-d",
    "dconf",
    "gvfsd",
    "tracker",
    "ibus",
    "pipewire",
    "wireplumber",
    "pulseaudio",
    "rtkit",
    "fwupd",
    "accounts-d",
    "colord",
    "upowerd",
    "power-prof",
    "switcheroo",
    "packagekit",
    "polkitd",
    "gcr",
    "gpg-agent",
    "ssh-agent",
    "unit",
    // Dremes stack: never close what we need.
    "waydroid",
    "scrcpy",
    "adb",
    "dremes",
    "obs",
    // Terminals / shells: closing mid-session is surprising.
    "gnome-terminal",
    "konsole",
    "kitty",
    "alacritty",
    "terminator",
    "xfce4-terminal",
    "xterm",
    "tmux",
    "bash",
    "zsh",
    "fish",
    "sh",
];

/// Self effective UID string for ownership filtering.
fn euid() -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2).map(|s| s.to_string())
}

/// Effective UID of a process (None if not readable / not ours).
fn proc_euid(pid: i32) -> Option<String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2).map(|s| s.to_string())
}

fn comm(pid: i32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Whether the process has a REAL graphical connection. A process that merely
/// inherited `DISPLAY` in its environment (shells, background tools) but has no
/// open X11/Wayland socket is NOT a GUI app and is never targeted.
fn connected_to_display(pid: i32) -> bool {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    for fd in fds.flatten() {
        let Ok(target) = std::fs::read_link(fd.path()) else { continue };
        let s = target.to_string_lossy();
        if s.contains(".X11-unix") || s.contains("wayland-") || s.starts_with("@/") {
            return true;
        }
    }
    false
}

fn is_protected(pid: i32) -> bool {
    let name = comm(pid).to_lowercase();
    let argline = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| s.replace('\0', " ").to_lowercase())
        .unwrap_or_default();
    PROTECTED.iter().any(|p| name.starts_with(p) || argline.contains(p))
}

fn ppid_of(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = stat.rfind(')')?;
    let mut parts = stat[close + 1..].split_whitespace();
    parts.next()?;
    parts.next()?.parse().ok()
}

/// Whether any ancestor of `pid` is a protected process (e.g. WebKit children
/// of the Dremes GUI, which never match `dremes` by their own name).
fn has_protected_ancestor(pid: i32) -> bool {
    let mut cur = pid;
    for _ in 0..16 {
        let parent = match ppid_of(cur) {
            Some(p) if p > 1 => p,
            _ => return false,
        };
        if is_protected(parent) {
            return true;
        }
        cur = parent;
    }
    false
}

/// Close the user's background GUI applications. Returns a human-readable note
/// describing what was closed and what was kept.
pub fn super_close() -> Result<String> {
    let uid = euid().ok_or_else(|| DremesError::msg("Could not determine the current user."))?;
    let self_pid = std::process::id() as i32;

    // Candidate GUI apps owned by this user.
    let mut targets: Vec<i32> = Vec::new();
    if let Ok(procs) = std::fs::read_dir("/proc") {
        for entry in procs.flatten() {
            let name = entry.file_name();
            let Some(pid_str) = name.to_str() else { continue };
            let Ok(pid) = pid_str.parse::<i32>() else { continue };
            if pid <= 0 || pid == self_pid {
                continue;
            }
            if proc_euid(pid).as_deref() != Some(uid.as_str()) {
                continue;
            }
            if !connected_to_display(pid) || is_protected(pid) || has_protected_ancestor(pid) {
                continue;
            }
            log::debug!("SUPER targeting pid={pid} comm={}", comm(pid));
            targets.push(pid);
        }
    }

    if targets.is_empty() {
        return Ok("SUPER found nothing to close — everything valuable is already running clean.".into());
    }

    // Two graceful rounds of SIGTERM. Never SIGKILL.
    for round in 0..2 {
        for pid in &targets {
            if std::path::Path::new(&format!("/proc/{pid}")).exists() {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
        }
        if round == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Report: group by unique process name.
    log::info!("SUPER targeted {} process(es)", targets.len());
    let mut closed: BTreeSet<String> = BTreeSet::new();
    let mut kept: BTreeSet<String> = BTreeSet::new();
    for pid in &targets {
        let name = comm(*pid);
        let name = if name.trim().is_empty() { "<unknown>".to_string() } else { name };
        if std::path::Path::new(&format!("/proc/{pid}")).exists() {
            kept.insert(name);
        } else {
            closed.insert(name);
        }
    }

    let closed_n = closed.len();
    let kept_n = kept.len();
    let closed_list: Vec<&str> = closed.iter().map(|s| s.as_str()).collect();
    let kept_list: Vec<&str> = kept.iter().map(|s| s.as_str()).collect();

    let mut note = format!(
        "SUPER closed {} app(s): {}",
        closed_n,
        if closed_list.is_empty() { "none".into() } else { closed_list.join(", ") }
    );
    if !kept_list.is_empty() {
        note.push_str(&format!(
            ". KEPT {} (refused to exit / still saving): {}",
            kept_n,
            kept_list.join(", ")
        ));
    }
    Ok(note)
}