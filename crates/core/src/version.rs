//! Dremes version information.

/// The semantic version of the application (MAJOR.MINOR.PATCH).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Human-readable application name.
pub const APP_NAME: &str = "Dremes";

/// Full display version string.
pub fn version_string() -> String {
    format!("{APP_NAME} {VERSION}")
}

/// An ISO-8601 UTC timestamp string for log events.
///
/// Computed from the system wall clock (no external subprocess). Returns an
/// ISO-8601 UTC string (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn now_iso8601() -> String {
    let secs = match std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
    {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    };
    iso8601_from_unix(secs)
}

/// Convert Unix epoch seconds to an ISO-8601 UTC string.
fn iso8601_from_unix(secs: u64) -> String {
    const DAY: u64 = 86_400;
    let days = secs / DAY;
    let rem = secs % DAY;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Howard Hinnant's civil-from-days algorithm (proleptic Gregorian).
fn civil_from_days(z0: u64) -> (u64, u64, u64) {
    let z = z0 + 719_468u64;
    let era = z / 146_097;
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; //  [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version must be major.minor.patch");
        for p in &parts {
            assert!(p.chars().all(|c| c.is_ascii_digit()), "version parts are numeric");
        }
    }

    #[test]
    #[cfg(unix)]
    fn timestamp_is_iso8601() {
        let ts = now_iso8601();
        assert!(!ts.is_empty(), "timestamp should not be empty");
        // ISO-8601 UTC: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20, "expected length-20 ISO-8601 timestamp, got {ts:?}");
        assert!(ts.ends_with("Z"), "timestamp should end with Z");
    }
}
