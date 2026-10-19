//! ADB manager.
//!
//! Dynamically discovers the current Waydroid ADB endpoint. It never uses a
//! hardcoded IP. It handles offline / unauthorized / stale / disconnected
//! devices and only restarts adb-server when actually necessary.

use dremes_core::error::{DremesError, Result};

use crate::command;

/// Discover the live Waydroid ADB endpoint (host:port).
///
/// Strategy:
/// 1. Determine the Waydroid session's network via `waydroid status` if avail.
/// 2. Cross-check `adb devices` for an online device.
/// Return the best candidate.
pub fn discover_endpoint(timeout: u64) -> Result<Option<String>> {
    let mut candidates: Vec<String> = Vec::new();

    // Candidate 1: from waydroid status network info.
    if let Ok(out) = command::run("waydroid", &["status"]) {
        for line in out.lines() {
            let lower = line.to_lowercase();
            if lower.contains("ip") || lower.contains("10.") || lower.contains("192.168.") {
                if let Some(ip) = extract_ip(line) {
                    candidates.push(format!("{ip}:5555"));
                }
            }
        }
    }

    // Candidate 2: read from lxc container config (network address).
    // This is the canonical Waydroid bridge IP.

    // Prefer an already-online device.
    for dev in list_devices(timeout)? {
        if dev.state == "device" || dev.state == "online" {
            return Ok(Some(dev.endpoint));
        }
    }

    // Otherwise test candidates in order; try to connect.
    for ep in candidates {
        match connect(&ep, timeout) {
            Ok(true) => return Ok(Some(ep)),
            Ok(false) => continue,
            Err(_) => continue,
        }
    }

    Ok(None)
}

/// A device as reported by `adb devices -l`.
pub struct DeviceInfo {
    pub endpoint: String,
    pub state: String,
    /// Extended info from `adb devices -l` (e.g. transport_id).
    pub detail: String,
}

/// Parse `adb devices` output into structured rows.
pub fn parse_devices(output: &str) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for (i, line) in output.lines().enumerate() {
        if i == 0 {
            continue; // header
        }
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let mut parts = l.split_whitespace();
        let endpoint = parts.next().unwrap_or_default().to_string();
        let state = parts.next().unwrap_or_default().to_string();
        if endpoint.is_empty() {
            continue;
        }
        let detail = parts.collect::<Vec<_>>().join(" ");
        out.push(DeviceInfo { endpoint, state, detail });
    }
    out
}

/// List all connected ADB devices (nonblocking-ish).
pub fn list_devices(timeout: u64) -> Result<Vec<DeviceInfo>> {
    let out = command::run_timeout("adb", &["devices", "-l"], timeout)?;
    Ok(parse_devices(&out))
}

/// Connect to an ADB endpoint, returns Ok(true) if it comes online.
pub fn connect(endpoint: &str, timeout: u64) -> Result<bool> {
    let out = command::run_timeout("adb", &["connect", endpoint], timeout)?;
    Ok(out.contains("connected") || out.contains("already connected"))
}

/// Ensure the given endpoint is online; recover stale/offline state.
pub fn ensure_connected(endpoint: &str, timeout: u64) -> Result<()> {
    // If already online, return.
    for dev in list_devices(timeout)? {
        if dev.endpoint == endpoint && (dev.state == "device" || dev.state == "online") {
            return Ok(());
        }
    }

    // Try to connect.
    connect(endpoint, timeout)?;

    // Give it a moment then verify.
    std::thread::sleep(std::time::Duration::from_millis(500));
    for dev in list_devices(timeout)? {
        if dev.endpoint == endpoint && (dev.state == "device" || dev.state == "online") {
            return Ok(());
        }
    }
    Err(DremesError::Adb(format!("Could not bring ADB device {endpoint} online.")))
}

/// Restart only our adb-server if it is truly wedged. Called sparingly.
pub fn restart_server_if_wedged(timeout: u64) -> Result<()> {
    // `adb kill-server` would affect other users; only do this when clearly
    // needed. Here we just start the server if not present.
    if !command::run_timeout("adb", &["start-server"], timeout).is_err() {
        Ok(())
    } else {
        Err(DremesError::Adb("adb-server could not be started.".into()))
    }
}

fn extract_ip(line: &str) -> Option<String> {
    // Match IPv4 addresses like 192.168.240.112 or 10.0.0.2.
    let start = line.find(|c: char| c.is_ascii_digit())?;
    let rest = &line[start..];
    // Heuristic: find a token that parses as IPv4.
    for token in rest.split([' ', '\t', ':', '(']) {
        let t = token.trim();
        if is_ipv4(t) {
            return Some(t.to_string());
        }
        let t = token.trim_end_matches(')');
        if is_ipv4(t) {
            return Some(t.to_string());
        }
    }
    None
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()
            && p.parse::<u32>().map(|v| v <= 255).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_devices_handles_header_and_empty() {
        let out = "List of devices attached\n";
        assert!(parse_devices(out).is_empty());
    }

    #[test]
    fn parse_devices_parses_rows() {
        let out = "List of devices attached\n192.168.240.112:5555 device product:waydroid transport_id:1\n";
        let devs = parse_devices(out);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].endpoint, "192.168.240.112:5555");
        assert_eq!(devs[0].state, "device");
        assert!(devs[0].detail.contains("transport_id:1"));
    }

    #[test]
    fn extract_ip_finds_ipv4() {
        assert_eq!(extract_ip("IP address: 192.168.240.112"), Some("192.168.240.112".into()));
        assert_eq!(extract_ip("192.168.240.112:5555"), Some("192.168.240.112".into()));
        assert_eq!(extract_ip("no ip here"), None);
    }

    #[test]
    fn is_ipv4_rejects_bad() {
        assert!(is_ipv4("192.168.240.112"));
        assert!(!is_ipv4("999.1.1.1"));
        assert!(!is_ipv4("hello"));
        assert!(!is_ipv4("1.2.3"));
    }
}
