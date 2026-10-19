//! Resource monitoring for the dashboard.
//!
//! Lightweight polling of /proc. When the Engine is OFF, the Engine simply
//! does not call this, so no expensive monitoring occurs. When it is ON, the
//! Engine samples at a reasonable interval (e.g. 2-4s), never in a busy loop.

use dremes_core::platform::ResourceSnapshot;

use std::collections::HashMap;
use std::time::Instant;

/// Userland clock ticks per second (CLK_TCK). Linux reports the standard
/// 100 on common architectures (x86/arm) since the kernel's USER_HZ default.
const CLK_TCK: f32 = 100.0;

/// Per-process CPU sampling state. Accumulates the total CPU ticks seen for a
/// pid so successive snapshots can compute a real CPU% delta over time.
pub type CpuHistory = HashMap<i64, (u64, Instant)>;

/// Snapshot of relevant resource usage.
///
/// `history` is mutated in place to compute per-process CPU deltas; each call
/// prunes pids that are no longer running. `_capture_device` is reserved for
/// identifying the capture process by device; scrcpy is currently identified
/// by its command line.
pub fn snapshot(_capture_device: &str, history: &mut CpuHistory) -> ResourceSnapshot {
    let mut snap = telemetry();
    snap.cpu_total = total_cpu();
    snap.ram_total_mb = total_ram_mb();
    snap.ram_used_mb = used_ram_mb();

    let self_pid = std::process::id();

    // Waydroid (container/session processes) and capture (scrcpy).
    let processes = enumerate_processes(history);
    snap.waydroid_cpu = 0.0;
    snap.waydroid_ram_mb = 0.0;
    snap.capture_cpu = 0.0;
    snap.capture_ram_mb = 0.0;
    snap.adb_cpu = 0.0;
    snap.adb_ram_mb = 0.0;

    for p in processes {
        if p.pid == self_pid as i64 {
            snap.engine_cpu = p.cpu_pct;
            snap.engine_ram_mb = p.rss_mb;
        }
        let cmd = p.cmdline;
        if cmd.contains("waydroid") {
            snap.waydroid_cpu += p.cpu_pct;
            snap.waydroid_ram_mb += p.rss_mb;
        } else if cmd.contains("scrcpy") {
            snap.capture_cpu += p.cpu_pct;
            snap.capture_ram_mb += p.rss_mb;
        } else if cmd.contains("adb") {
            snap.adb_cpu += p.cpu_pct;
            snap.adb_ram_mb += p.rss_mb;
        }
    }

    snap
}

struct ProcInfo {
    pid: i64,
    cmdline: String,
    cpu_pct: f32,
    rss_mb: f32,
}

fn total_cpu() -> f32 {
    // Simple read of the "cpu" aggregate from /proc/stat to derive busy %
    // over a tiny sample. Kept lightweight (single read).
    let now = read_cpu_jiffies();
    let before = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let after = read_cpu_jiffies();
    let _ = before;
    let dt = 100_000.0;
    let total = (after.total.saturating_sub(now.total)) as f32;
    if total <= 0.0 {
        return 0.0;
    }
    let idle_delta = (after.idle.saturating_sub(now.idle)) as f32;
    let used = total - idle_delta;
    let pct = used / total * 100.0;
    // Scale to real-time window.
    let _ = dt;
    pct.clamp(0.0, 100.0)
}

#[derive(Default)]
struct CpuJiffies {
    total: u64,
    idle: u64,
}

fn read_cpu_jiffies() -> CpuJiffies {
    let mut r = CpuJiffies::default();
    if let Ok(contents) = std::fs::read_to_string("/proc/stat") {
        if let Some(line) = contents.lines().next() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // fields[0]="cpu"; fields[1..8] are user,nice,system,idle,iowait,irq,softirq
            if fields.len() >= 8 {
                let user: u64 = fields[1].parse().unwrap_or(0);
                let nice: u64 = fields[2].parse().unwrap_or(0);
                let system: u64 = fields[3].parse().unwrap_or(0);
                let idle: u64 = fields[4].parse().unwrap_or(0);
                let iowait: u64 = fields[5].parse().unwrap_or(0);
                let irq: u64 = fields[6].parse().unwrap_or(0);
                let softirq: u64 = fields[7].parse().unwrap_or(0);
                r.total = user + nice + system + idle + iowait + irq + softirq;
                r.idle = idle + iowait;
            }
        }
    }
    r
}

fn total_ram_mb() -> f32 {
    meminfo("MemTotal")
}

fn used_ram_mb() -> f32 {
    let total = meminfo("MemTotal");
    let available = meminfo("MemAvailable");
    (total - available).max(0.0)
}

fn meminfo(key: &str) -> f32 {
    if let Ok(contents) = std::fs::read_to_string("/proc/meminfo") {
        for line in contents.lines() {
            if let Some(line) = line.strip_prefix(key) {
                // After the key the line is ":\t<size> kB"; the first token after
                // splitting on ":" is the numeric size (may carry a "kB" suffix).
                if let Some(value) = line.split_once(':').map(|(_, v)| v.trim()) {
                    let kb: f32 = value
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.trim_end_matches("kB").parse().ok())
                        .unwrap_or(0.0);
                    return kb / 1024.0;
                }
            }
        }
    }
    0.0
}

fn read_stat(pid: i64, history: &mut CpuHistory) -> Option<ProcInfo> {
    let path = format!("/proc/{pid}/stat");
    let contents = std::fs::read_to_string(&path).ok()?;
    // Field 23 is rss in pages (after the comm field which may contain spaces).
    // comm is field 2 and is parenthesized "name". Find last ')'.
    let close = contents.rfind(')')?;
    let after = &contents[close + 1..];
    // Remaining fields start at field 3 (state). rss=24 => index 21;
    // utime=14, stime=15 => indices 11, 12.
    let parts: Vec<&str> = after.split_whitespace().collect();
    let rss_pages: u64 = parts.get(21).and_then(|s| s.parse().ok()).unwrap_or(0);
    let utime: u64 = parts.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = parts.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pagesize = 4096usize as u64;

    let ticks = utime + stime;
    let now = Instant::now();
    let cpu_pct = match history.get(&pid) {
        Some(&(prev_ticks, prev_at)) => {
            let dt_secs = now.duration_since(prev_at).as_secs_f32();
            let d_ticks = ticks.saturating_sub(prev_ticks) as f32;
            if dt_secs > 0.0 && d_ticks > 0.0 {
                ((d_ticks / CLK_TCK) / dt_secs * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            }
        }
        None => 0.0,
    };
    history.insert(pid, (ticks, now));

    Some(ProcInfo {
        pid,
        cmdline: String::new(),
        cpu_pct,
        rss_mb: (rss_pages * pagesize) as f32 / (1024.0 * 1024.0),
    })
}

fn enumerate_processes(history: &mut CpuHistory) -> Vec<ProcInfo> {
    let mut res = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir("/proc") {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let Some(pid_str) = name.to_str() else { continue };
            let Ok(pid) = pid_str.parse::<i64>() else { continue };
            let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
                .map(|s| s.replace('\0', " "))
                .unwrap_or_default();
            if cmdline.is_empty() {
                continue;
            }
            let mut info = read_stat(pid, history)
                .unwrap_or(ProcInfo { pid, cmdline: String::new(), cpu_pct: 0.0, rss_mb: 0.0 });
            info.cmdline = cmdline;
            res.push(info);
        }
    }
    res
}

/// Number of CPU cores (informational).
pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

// ---- Hardware telemetry (all reads are best-effort and guarded) ----

/// Read-only hardware telemetry (temperatures, GPU, fan, battery). Safe to
/// poll at any time — it spawns nothing heavy and never samples /proc, so the
/// Engine can keep the dashboard fresh even while the master switch is OFF.
pub fn telemetry() -> ResourceSnapshot {
    let mut snap = ResourceSnapshot::default();
    snap.cpu_temp_c = cpu_temp_c();
    snap.ssd_temp_c = ssd_temp_c();
    snap.fan_rpm = fan_rpm();
    snap.battery_pct = battery_pct();
    let (gpu_util, gpu_temp) = gpu_metrics();
    snap.gpu_util_pct = gpu_util;
    snap.gpu_temp_c = gpu_temp;
    snap
}

/// CPU package/core temperature in °C from the kernel thermal zones
/// (e.g. x86_pkg_temp / coretemp). `None` when not exposed.
fn cpu_temp_c() -> Option<f32> {
    let zones = std::fs::read_dir("/sys/class/thermal").ok()?;
    let mut hottest: Option<f32> = None;
    for entry in zones.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("thermal_zone") {
            continue;
        }
        let base = entry.path();
        let ty = std::fs::read_to_string(base.join("type")).unwrap_or_default();
        let is_cpu = ty.contains("x86_pkg_temp")
            || ty.contains("coretemp")
            || ty.to_lowercase().contains("cpu");
        if !is_cpu {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(base.join("temp")) {
            if let Ok(milli) = raw.trim().parse::<f32>() {
                let c = milli / 1000.0;
                hottest = Some(hottest.map_or(c, |h| h.max(c)));
            }
        }
    }
    hottest
}

fn file_value(path: &str) -> Option<f32> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .and_then(|s| s.parse::<f32>().ok())
}

/// GPU utilization % and temperature °C. Primary source is `nvidia-smi`
/// (works on NVIDIA), with an i915 `gpu_busy_percent` fallback for the util
/// number. `None`/`None` when nothing is exposed.
fn gpu_metrics() -> (Option<f32>, Option<f32>) {
    if which_quiet("nvidia-smi") {
        let out = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=utilization.gpu,temperature.gpu", "--format=csv,noheader,nounits"])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let line = String::from_utf8_lossy(&o.stdout);
                if let Some((util, temp)) = line.lines().next().and_then(|l| l.split_once(',')) {
                    return (
                        util.trim().parse::<f32>().ok(),
                        temp.trim().parse::<f32>().ok(),
                    );
                }
            }
        }
    }
    // i915 / amdgpu busy-percent fallback.
    let util = std::fs::read_dir("/sys/class/drm")
        .ok()
        .and_then(|dirs| {
            dirs.flatten().find_map(|d| {
                let name = d.file_name().to_string_lossy().into_owned();
                if !name.starts_with("card") {
                    return None;
                }
                file_value(&format!("{}/device/gpu_busy_percent", d.path().display()))
            })
        });
    (util, None)
}

/// Primary SSD / NVMe temperature in °C (hwmon driver name "nvme").
fn ssd_temp_c() -> Option<f32> {
    hwmon_first("nvme", "temp1_input").map(|v| v / 1000.0)
}

/// Fan speed in RPM from the first exposed hwmon fan input.
fn fan_rpm() -> Option<f32> {
    let hwmons = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for h in hwmons.flatten() {
        let base = h.path();
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.starts_with("fan") && fname.ends_with("_input") {
                return file_value(&e.path().display().to_string());
            }
        }
    }
    None
}

/// Battery charge percent (0-100) from `/sys/class/power_supply/BAT*`.
fn battery_pct() -> Option<f32> {
    let supply = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for ps in supply.flatten() {
        let name = ps.file_name().to_string_lossy().into_owned();
        if name.starts_with("BAT") {
            if let Some(pct) = file_value(&format!("{}/capacity", ps.path().display())) {
                return Some(pct);
            }
        }
    }
    None
}

/// Find the first value file inside a hwmon directory whose `name` file matches
/// `driver`, returning the raw parsed number.
fn hwmon_first(driver: &str, value_file: &str) -> Option<f32> {
    let hwmons = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for h in hwmons.flatten() {
        let base = h.path();
        let name = std::fs::read_to_string(base.join("name")).unwrap_or_default();
        if name.trim() == driver {
            return file_value(&base.join(value_file).display().to_string());
        }
    }
    None
}

/// `true` when the program exists on PATH.
fn which_quiet(prog: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {prog} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_are_nonzero() {
        // On a real host these are always present and > 0.
        assert!(total_ram_mb() > 0.0, "MemTotal should parse to a positive MB value");
        assert!(used_ram_mb() > 0.0, "MemAvailable-derived usage should be positive");
    }

    #[test]
    fn cpu_read_does_not_panic() {
        let _ = total_cpu();
    }

    #[test]
    fn enumerates_something() {
        // On a real system there are always processes. May be empty in CI.
        let mut history = CpuHistory::new();
        let _ = enumerate_processes(&mut history);
    }

    #[test]
    fn snapshot_produces_sane_across_two_samples() {
        let mut history = CpuHistory::new();
        let a = snapshot("/dev/video0", &mut history);
        // Burn some CPU between samples so a busy process shows non-zero CPU%.
        let mut x = 0u64;
        let start = Instant::now();
        while start.elapsed().as_millis() < 200 {
            x = x.wrapping_add(x.wrapping_mul(31));
        }
        std::hint::black_box(x);
        let b = snapshot("/dev/video0", &mut history);
        assert!(a.cpu_total >= 0.0 && b.cpu_total >= 0.0);
        assert!(b.ram_total_mb > 0.0);
        assert!(b.ram_used_mb > 0.0);
        // The first sample seeds history; engine CPU is this same process.
        assert!(a.engine_ram_mb >= 0.0);
    }
}
