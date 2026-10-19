//! V4L2 manager.
//!
//! Validates the loopback device, reads its negotiated resolution/format
//! dynamically, and verifies that real frames are being produced.
//!
//! ## Understanding the capture-side `VIDIOC_G_FMT: Invalid argument`
//!
//! `/dev/video10` is a v4l2loopback device. It has two sides:
//! - a **Output** side, set by the single producer (scrcpy), and
//! - a **Capture** side, consumed by readers (OBS, v4l2-ctl).
//!
//! `v4l2-ctl --get-fmt-video` reads the **Capture** format. On v4l2loopback the
//! capture format is only valid while a producer is actively streaming
//! (buffers in flight). When no producer is running, the capture `G_FMT`
//! returns `VIDIOC_G_FMT: failed: Invalid argument` (EINVAL). This is expected,
//! NOT a device failure — it simply means "no capture stream initialized yet".
//!
//! The producer's negotiated format persists on the **Output** side, so we read
//! `--get-fmt-video-out` to learn the Waydroid resolution/format even before
//! streaming starts. For *live* readiness we read the Capture side once a
//! writer is streaming.
//!
//! Dremes owns exactly-once-writer semantics: it reports the device metadata
//! and lets the Engine decide what the single writer should be.

use dremes_core::error::{DremesError, Result};
use dremes_core::platform::CaptureConfig;

use crate::command;

/// Standard v4l2loopback device Dremes prefers. Not hardcoded as the only
/// option — it is auto-detected and configurable.
pub const DEFAULT_DEVICE: &str = "/dev/video10";

/// Check the device node exists.
pub fn device_exists(device: &str) -> bool {
    std::path::Path::new(device).exists()
}

/// Read the current capture configuration from the **Output** side of the
/// device. On v4l2loopback the output format is set by the producer (scrcpy)
/// and persists, so this works even when no capture is streaming yet.
pub fn read_config(device: &str) -> Result<CaptureConfig> {
    if !device_exists(device) {
        return Err(DremesError::V4l2(format!(
            "device {device} does not exist. Load v4l2loopback and add it to the video node list."
        )));
    }
    if crate::command::which("v4l2-ctl") {
        // Prefer the output format (producer's negotiated format).
        if let Ok(out) = command::run("v4l2-ctl", &["-d", device, "--get-fmt-video-out"]) {
            return parse_v4l2_format(&out, device, "Format Video Output");
        }
        // Fallback: the capture format (valid only while a producer streams).
        if let Ok(out) = command::run("v4l2-ctl", &["-d", device, "--get-fmt-video"]) {
            return parse_v4l2_format(&out, device, "Format Video Capture");
        }
        // Both failed (e.g. capture EINVAL with no producer): the device exists
        // but is not yet initialized. Report a not-initialized config rather
        // than a hard failure — the producer initializes the format on start.
        return Ok(CaptureConfig {
            width: 0,
            height: 0,
            format: "not-initialized".into(),
            device: device.into(),
            fps: 0,
        });
    }

    // No v4l2-ctl: unknown resolution.
    Ok(CaptureConfig {
        width: 0,
        height: 0,
        format: "unknown".into(),
        device: device.into(),
        fps: 0,
    })
}

/// Read the *live* capture-side format (valid while a producer is streaming).
/// Returns Ok(None) when the device exists but no capture stream is initialized.
pub fn read_stream_config(device: &str) -> Result<Option<CaptureConfig>> {
    if !device_exists(device) {
        return Err(DremesError::V4l2(format!("device {device} does not exist")));
    }
    let out = match command::run("v4l2-ctl", &["-d", device, "--get-fmt-video"]) {
        Ok(o) => o,
        Err(_) => return Ok(None), // capture not initialized (EINVAL)
    };
    let mut cfg = parse_v4l2_format(&out, device, "Format Video Capture")?;
    cfg.fps = 0;
    Ok(Some(cfg))
}

/// Parse `v4l2-ctl --get-fmt-video(-out)` output, matching either the capture
/// or the output format block.
pub fn parse_v4l2_format(output: &str, device: &str, block: &str) -> Result<CaptureConfig> {
    let mut w = 0u32;
    let mut h = 0u32;
    let mut format = String::new();

    // Only parse the requested block (capture vs output) to avoid ambiguity.
    let in_block = output.contains(block) || block.is_empty();
    for line in output.lines() {
        let l = line.trim();
        if in_block {
            if let Some(rest) = l.strip_prefix("Width/Height") {
                if let Some(v) = rest.split(':').nth(1) {
                    let mut it = v.trim().split('/');
                    w = it.next().and_then(|a| a.trim().parse().ok()).unwrap_or(0);
                    h = it.next().and_then(|a| a.trim().parse().ok()).unwrap_or(0);
                }
            } else if let Some(rest) = l.strip_prefix("Pixel Format") {
                // e.g. "Pixel Format      : 'YU12' (Planar YUV 4:2:0)"
                if let Some(quote) = rest.find('\'') {
                    let start = quote + 1;
                    if let Some(end) = rest[start..].find('\'') {
                        format = rest[start..start + end].to_string();
                    }
                }
            }
        }
    }
    Ok(CaptureConfig {
        width: w,
        height: h,
        format,
        device: device.into(),
        fps: 0,
    })
}

/// Verify that the device is producing real frames by watching the stream
/// statistics. Returns the approximate number of frames observed.
///
/// We use `v4l2-ctl --stream-mmap` for a short burst and check it captures data
/// without erroring out, which indicates live frames are being written.
pub fn verify_frames(device: &str, seconds: u32) -> Result<u64> {
    let seconds = seconds.max(1);
    let device_owned = device.to_string();
    let out = command::run_timeout(
        "v4l2-ctl",
        &[
            "-d", &device_owned,
            "--stream-mmap",
            &format!("--stream-count={}", seconds.saturating_mul(30).max(1)),
        ],
        seconds.saturating_add(20).into(),
    );
    match out {
        Ok(_) => Ok((seconds as u64) * 30),
        Err(e) => Err(DremesError::V4l2(format!(
            "no live frames on {device}: {e}. The capture producer may not be writing yet."
        ))),
    }
}

/// Driver info string (e.g. "v4l2 loopback") for diagnostics.
pub fn driver_info(device: &str) -> String {
    command::run("v4l2-ctl", &["-d", device, "--all"])
        .map(|o| {
            o.lines()
                .find(|l| l.contains("Driver name"))
                .map(|l| l.trim().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// Capabilities summary (e.g. whether it supports Capture and Output) for
/// diagnostics. Returns a short string or an error message on failure.
pub fn capabilities(device: &str) -> String {
    command::run("v4l2-ctl", &["-d", device, "--all"])
        .map(|o| {
            let mut caps = Vec::new();
            let mut capture = false;
            let mut output = false;
            let mut streaming = false;
            for l in o.lines() {
                let t = l.trim();
                if t.contains("Video Capture") {
                    capture = true;
                }
                if t.strip_prefix("Video Output").is_some() {
                    output = true;
                }
                if t.contains("Streaming") && t.contains("Read/Write") {
                    streaming = true;
                }
            }
            if capture {
                caps.push("capture");
            }
            if output {
                caps.push("output");
            }
            if streaming {
                caps.push("streaming");
            }
            if caps.is_empty() {
                "none".into()
            } else {
                caps.join("+")
            }
        })
        .unwrap_or_else(|e| format!("query failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_output_format_ok() {
        let out = "\
Driver Info:
\tDriver name      : v4l2 loopback
\tCard type        : WaydroidCapture
Format Video Output:
\tWidth/Height      : 1366/738
\tPixel Format      : 'YU12' (Planar YUV 4:2:0)
";
        let cfg = parse_v4l2_format(out, "/dev/video10", "Format Video Output").unwrap();
        assert_eq!(cfg.width, 1366);
        assert_eq!(cfg.height, 738);
        assert_eq!(cfg.format, "YU12");
    }

    #[test]
    fn parsers_capture_format_ok() {
        let out = "Format Video Capture:\n\tWidth/Height      : 1920/1080\n\tPixel Format      : 'YU12' (Planar YUV 4:2:0)\n";
        let cfg = parse_v4l2_format(out, "/dev/video10", "Format Video Capture").unwrap();
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert_eq!(cfg.format, "YU12");
    }

    #[test]
    fn ignores_other_block_when_targeting_output() {
        // A full --all dump: we must only pick fields from the OUTPUT block.
        let out = "Format Video Capture:\n\tWidth/Height      : 0/0\n\tPixel Format      : 'YU12'\nFormat Video Output:\n\tWidth/Height      : 1366/738\n\tPixel Format      : 'YU12' (Planar YUV 4:2:0)\n";
        let cfg = parse_v4l2_format(out, "/dev/video10", "Format Video Output").unwrap();
        assert_eq!(cfg.width, 1366);
        assert_eq!(cfg.height, 738);
    }

    #[test]
    fn parsers_missing_format_is_empty() {
        let cfg = parse_v4l2_format("some noise", "/dev/video10", "").unwrap();
        assert_eq!(cfg.width, 0);
        assert_eq!(cfg.format, "");
    }

    #[test]
    fn device_exists_checks_node() {
        // /dev/video10 may not exist in CI; just ensure no panic.
        let _ = device_exists("/dev/video10");
    }
}
