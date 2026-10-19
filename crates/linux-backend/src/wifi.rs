//! WiFi booster: disable/enable wireless power-save for lower latency.
//!
//! Two transport strategies are attempted, in order:
//!
//! 1. **NetworkManager** (`nmcli`) — works from an unprivileged user session via
//!    the user's own polkit policy. The `802-11-wireless.powersave` connection
//!    property is set to `2` (disable) to boost, and restored to the value seen
//!    before boosting when toggled off.
//! 2. **`iw`** — the raw kernel-netlink path. This usually requires CAP_NET_ADMIN
//!    (root), so it is only a fallback for systems without NetworkManager; if the
//!    kernel refuses the write we report the error truthfully.
//!
//! The engine never guesses: the note returned to the GUI describes exactly what
//! was applied (or why it could not be).

use dremes_core::error::{DremesError, Result};
use dremes_core::platform::WifiInfo;

/// The original NM power-save value captured when boost was enabled, so turning
/// boost off restores exactly what was there before (0=default, 1=ignore,
/// 2=disable, 3=enable). `None` when boost has not been enabled this session.
static ORIGINAL: std::sync::Mutex<Option<i32>> = std::sync::Mutex::new(None);

/// Discover the active wireless interface via sysfs (`/sys/class/net/*/wireless`).
pub fn iface() -> Option<String> {
    let net = std::fs::read_dir("/sys/class/net").ok()?;
    for entry in net.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if std::fs::metadata(format!("/sys/class/net/{name}/wireless")).is_ok() {
            return Some(name);
        }
    }
    None
}

/// Whether the given interface currently has power-save enabled (via `iw`).
pub fn power_save_on(iface: &str) -> bool {
    let out = std::process::Command::new("iw")
        .args(["dev", iface, "get", "power_save"])
        .output()
        .ok();
    match out {
        Some(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_ascii_lowercase()))
                .any(|v| v == "on")
        }
        _ => false,
    }
}

/// A `WifiInfo` probe for the current wireless interface.
pub fn info() -> Option<WifiInfo> {
    let iface = iface()?;
    Some(WifiInfo {
        power_save_on: power_save_on(&iface),
        iface,
    })
}

/// The active NetworkManager wireless connection name (e.g. "HomeWiFi").
fn nm_active_wifi_conn() -> Option<String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "TYPE,NAME", "connection", "show", "--active"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().find_map(|line| {
        let mut parts = line.splitn(2, ':');
        match (parts.next(), parts.next()) {
            (Some("802-11-wireless"), Some(name)) if !name.is_empty() => Some(name.to_string()),
            _ => None,
        }
    })
}

/// Current NM power-save setting for a connection (0=default, 2=disable).
fn nm_powersave(conn: &str) -> Option<i32> {
    let out = std::process::Command::new("nmcli")
        .args(["-g", "802-11-wireless.powersave", "connection", "show", conn])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
    match value.as_str() {
        "default" => Some(0),
        "ignore" => Some(1),
        "disable" => Some(2),
        "enable" => Some(3),
        _ => value.parse::<i32>().ok(),
    }
}

/// Apply an NM power-save value and re-activate the connection so it takes
/// effect immediately.
fn nm_set_powersave(conn: &str, value: i32) -> bool {
    let modify = std::process::Command::new("nmcli")
        .args(["connection", "modify", conn, "802-11-wireless.powersave", &value.to_string()])
        .output();
    if !matches!(modify, Ok(o) if o.status.success()) {
        return false;
    }
    std::process::Command::new("nmcli")
        .args(["connection", "up", conn])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Toggle the WiFi boost. `on` disables wireless power-save (lower latency);
/// `off` restores the previous state. Returns the human-readable result note.
pub fn boost(on: bool) -> Result<String> {
    let iface = iface().ok_or_else(|| DremesError::msg("No wireless interface detected."))?;

    // Preferred path: NetworkManager (no root required).
    if let Some(conn) = nm_active_wifi_conn() {
        if on {
            let current = nm_powersave(&conn).unwrap_or(0);
            *ORIGINAL.lock().unwrap() = Some(current);
            if nm_set_powersave(&conn, 2) {
                Ok(format!("WiFi power-save disabled on {iface} (lower latency)"))
            } else {
                Err(DremesError::msg(format!(
                    "Could not apply WiFi power-save change on {iface}."
                )))
            }
        } else {
            let restore = ORIGINAL.lock().unwrap().take().unwrap_or(0);
            if nm_set_powersave(&conn, restore) {
                Ok(format!("WiFi power-save restored on {iface}"))
            } else {
                Err(DremesError::msg(format!(
                    "Could not restore WiFi power-save on {iface}."
                )))
            }
        }
    } else {
        // Fallback: raw iw (usually needs root).
        let plink = if on { "off" } else { "on" };
        let cmd = format!("iw dev {iface} set power_save {plink}");
        match std::process::Command::new("sh").arg("-c").arg(&cmd).status() {
            Ok(s) if s.success() => Ok(if on {
                format!("WiFi power-save disabled on {iface} (lower latency)")
            } else {
                format!("WiFi power-save enabled on {iface}")
            }),
            _ => Err(DremesError::msg(format!(
                "Cannot toggle wireless power-save on {iface}: NetworkManager is not \
                 available and changing it directly requires root."
            ))),
        }
    }
}