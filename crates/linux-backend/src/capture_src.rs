//! General-purpose capture sources.
//!
//! Beyond the dedicated Waydroid → scrcpy → /dev/video10 pipeline, Dremes can
//! write any source into a free loopback V4L2 device (a "Dremes capture
//! device") that OBS or any V4L2 consumer can read:
//!
//! - **Mobile**: scrcpy over ADB (`scrcpy --v4l2-sink <out>`)
//! - **Camera / V4L2 device**: ffmpeg V4L2 passthrough
//! - **Monitor (X11)**: ffmpeg `x11grab`
//! - **Monitor (Wayland)**: `wf-recorder` (optional backend; honest "missing"
//!   message when not installed)
//! - **Window (X11)**: `xdotool` geometry + ffmpeg `x11grab` region
//! - **Window (Wayland)**: honest guidance (use OBS Studio Window Capture)
//! - **Game**: guidance (needs OBS Studio + OBS-VKCapture Vulkan layer)
//!
//! Invariants enforced here, matching the release spec:
//! - The reserved Waydroid device is *never* used as a general output.
//! - Optional backends are checked up-front and produce a graceful
//!   "backend unavailable" error instead of a broken capture.
//! - Every spawned process is tracked (pid + binary), and stopping it verifies
//!   the pid's command line still matches before signaling, so a recycled pid
//!   is never touched.

use std::path::Path;
use std::process::{Command, Stdio};

use dremes_core::capture::{
    CaptureBackendStatus, CaptureDiagnosis, CaptureRequest, CaptureSourceKind,
    GeneralCaptureHandle, MobileDeviceInfo, SessionInfo, V4l2DeviceInfo, is_reserved_device,
};
use dremes_core::error::{DremesError, Result};
use dremes_core::platform::GeneralCaptureStart;

use crate::{command, v4l2};

/// Detect the desktop/session environment from the ambient environment.
pub fn session_info() -> SessionInfo {
    SessionInfo::detect(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

/// Honest status of every optional capture-producing backend on this host.
pub fn backend_status(session: &SessionInfo) -> CaptureBackendStatus {
    let scrcpy = command::which("scrcpy");
    let adb = command::which("adb");
    let ffmpeg = command::which("ffmpeg");
    let wf_recorder = command::which("wf-recorder");
    let xdotool = command::which("xdotool");
    let vkcapture = has_vkcapture_layer();

    let mut notes = vec![
        format!("scrcpy: {} (Mobile capture)", present(scrcpy)),
        format!("adb: {} (Mobile capture)", present(adb)),
        format!("ffmpeg: {} (Camera / V4L2 / X11 monitor & window)", present(ffmpeg)),
        format!("xdotool: {} (X11 window geometry)", present(xdotool)),
        format!("OBS-VKCapture layer: {} (Game capture via OBS)", present(vkcapture)),
    ];
    if session.wayland {
        notes.push(format!("wf-recorder: {} (Wayland monitor capture)", present(wf_recorder)));
        notes.push("Native window capture under Wayland is provided by OBS Studio's Window Capture.".into());
    }

    let available = (scrcpy && adb) || ffmpeg || wf_recorder;
    CaptureBackendStatus { available, notes }
}

fn present(ok: bool) -> &'static str {
    if ok { "installed" } else { "missing (optional)" }
}

/// Whether a source kind has a capture method that can start right now.
pub fn source_available(kind: CaptureSourceKind, session: &SessionInfo) -> (bool, String) {
    match kind {
        CaptureSourceKind::Mobile => {
            if !command::which("scrcpy") {
                (false, "scrcpy is not installed (optional Mobile-capture backend).".into())
            } else if !command::which("adb") {
                (false, "adb is not installed (required for Mobile capture).".into())
            } else {
                (true, String::new())
            }
        }
        CaptureSourceKind::Game => {
            if has_vkcapture_layer() {
                (true, "Start OBS Studio and add a Game Capture source as usual.".into())
            } else {
                (false, "Game capture needs the optional OBS-VKCapture Vulkan layer and OBS Studio.".into())
            }
        }
        CaptureSourceKind::Window => {
            if session.wayland {
                (false, "Native window capture under Wayland uses OBS Studio's Window Capture. Dremes cannot grab a specific window on Wayland directly.".into())
            } else if !command::which("ffmpeg") || !command::which("xdotool") {
                (false, "Window capture (X11) needs ffmpeg and xdotool (optional backends).".into())
            } else {
                (true, String::new())
            }
        }
        CaptureSourceKind::Monitor => {
            if session.wayland {
                if command::which("wf-recorder") {
                    (true, String::new())
                } else {
                    (false, "Monitor capture under Wayland needs wf-recorder (wlroots/LabWC compositors). Install wf-recorder or use OBS Studio Screen Capture.".into())
                }
            } else if command::which("ffmpeg") {
                (true, String::new())
            } else {
                (false, "Monitor capture needs ffmpeg (optional backend).".into())
            }
        }
        CaptureSourceKind::Camera | CaptureSourceKind::V4l2Device => {
            if command::which("ffmpeg") {
                (true, String::new())
            } else {
                (false, "Camera capture needs ffmpeg (optional backend).".into())
            }
        }
    }
}

/// Enumerate all V4L2 devices on the host from sysfs (cheap, no subprocess).
///
/// `/dev/video10` and any device matching `reserved` are flagged `is_reserved`
/// and excluded from "free output devices".
pub fn list_v4l2_devices(reserved: &str) -> Vec<V4l2DeviceInfo> {
    let mut out: Vec<V4l2DeviceInfo> = Vec::new();
    let base = Path::new("/sys/class/video4linux");
    let Ok(rd) = std::fs::read_dir(base) else {
        log::debug!("{base:?} not readable; no V4L2 devices enumerated");
        return out;
    };
    for entry in rd.flatten() {
        let dir = entry.path();
        let Some(dir_name) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(index) = dir_name.strip_prefix("video") else { continue };
        let Ok(index): std::result::Result<i32, _> = index.parse() else { continue };
        let node = format!("/dev/video{index}");
        if !Path::new(&node).exists() {
            continue; // sysfs entry without a node (unplugged / unused)
        }
        let name = std::fs::read_to_string(dir.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let lower = name.to_lowercase();
        // Robust loopback detection: v4l2loopback exposes a `format` sysfs
        // file that real capture devices (uvcvideo, etc.) never have. The card
        // name is unreliable — admins label nodes arbitrarily ("WaydroidCapture",
        // "GeneralCapture", "game", ...) — so we rely on the marker first and
        // keep the name keyword as a fallback for oddly-sysfs'd setups.
        let is_loopback = dir.join("format").exists()
            || lower.contains("loopback")
            || lower.contains("v4l2loopback");
        out.push(V4l2DeviceInfo {
            path: node.clone(),
            name,
            is_loopback,
            is_capture: true,
            is_output: is_loopback,
            is_reserved: node == reserved,
        });
    }
    out.sort_by_key(|d| {
        d.path
            .trim_start_matches("/dev/video")
            .parse::<i32>()
            .unwrap_or(i32::MAX)
    });
    out
}

/// List online/offline mobile (ADB) devices. Empty when adb is absent.
pub fn mobile_devices() -> Vec<MobileDeviceInfo> {
    if !command::which("adb") {
        return Vec::new();
    }
    match command::run_timeout("adb", &["devices", "-l"], 10) {
        Ok(out) => parse_mobile_devices(&out),
        Err(_) => Vec::new(),
    }
}

/// Parse `adb devices -l` output into structured rows.
pub fn parse_mobile_devices(output: &str) -> Vec<MobileDeviceInfo> {
    let mut out = Vec::new();
    for line in output.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let serial = parts.next().unwrap_or_default().to_string();
        if serial.is_empty() {
            continue;
        }
        let state = parts.next().unwrap_or_default().to_string();
        let mut product = String::new();
        let mut model = String::new();
        let mut transport = String::new();
        for tok in parts {
            if let Some(v) = tok.strip_prefix("product:") {
                product = v.to_string();
            } else if let Some(v) = tok.strip_prefix("model:") {
                model = v.to_string();
            } else if let Some(v) = tok.strip_prefix("transport_id:") {
                transport = v.to_string();
            }
        }
        out.push(MobileDeviceInfo {
            serial,
            state: state.clone(),
            product,
            model,
            transport,
            online: state == "device",
        });
    }
    out
}

/// Start a general-purpose capture from a request. The reserved Waydroid node
/// is never used as the output.
pub fn start(request: &CaptureRequest, reserved: &str) -> Result<GeneralCaptureStart> {
    let session = session_info();
    if !session.wayland && session.session_type == "unknown" && session.display.is_empty() {
        log::debug!("Session environment is ambiguous; capture may fail with a clear message.");
    }
    if is_reserved_device(&request.output_device, reserved) {
        return Err(DremesError::Capture(format!(
            "{} is reserved for the Waydroid capture pipeline and cannot be used by general capture.",
            request.output_device
        )));
    }
    match request.kind {
        CaptureSourceKind::Mobile => start_mobile(request, reserved),
        CaptureSourceKind::Camera | CaptureSourceKind::V4l2Device => start_v4l2_source(request, reserved, &session),
        CaptureSourceKind::Monitor => start_monitor(request, reserved, &session),
        CaptureSourceKind::Window => start_window(request, reserved, &session),
        CaptureSourceKind::Game => Err(game_guidance()),
    }
}

// ---- Output device selection ----

/// Resolve the concrete output device for a request, never the reserved node.
fn resolve_output(request: &CaptureRequest, reserved: &str) -> Result<String> {
    let requested = request.output_device.trim();
    if !requested.is_empty() {
        if is_reserved_device(requested, reserved) {
            return Err(DremesError::Capture(
                "The reserved Waydroid capture device cannot be used as a general capture output.".into(),
            ));
        }
        if !v4l2::device_exists(requested) {
            return Err(DremesError::V4l2(format!(
                "output device {requested} does not exist. Load v4l2loopback with extra video nodes."
            )));
        }
        return Ok(requested.to_string());
    }
    let first_free = list_v4l2_devices(reserved)
        .into_iter()
        .find(|d| !d.is_reserved && d.is_output)
        .map(|d| d.path)
        .unwrap_or_default();
    if first_free.is_empty() {
        return Err(DremesError::Capture(
            "No free loopback V4L2 output device is available. Load v4l2loopback with an extra video node (e.g. video_nr=10,11) so general capture can write somewhere OBS can read.".into(),
        ));
    }
    Ok(first_free)
}

/// The first non-loopback V4L2 source (best-guess camera), or none.
fn first_camera_source() -> Option<String> {
    list_v4l2_devices("").into_iter().find(|d| !d.is_loopback).map(|d| d.path)
}

/// Capture-stack health report. Read-only and cheap; never spawns anything.
pub fn diagnosis(reserved: &str) -> CaptureDiagnosis {
    let mut issues = Vec::new();
    let mut notes = Vec::new();
    let mut root_actions = Vec::new();

    // 1. Kernel module
    let module = std::fs::read_to_string("/proc/modules")
        .map(|s| s.lines().any(|l| l.starts_with("v4l2loopback ")))
        .unwrap_or(false);
    if module {
        notes.push("v4l2loopback kernel module is loaded.".into());
    } else {
        issues.push(
            "The v4l2loopback kernel module is not loaded, so no loopback output can exist.".into(),
        );
        root_actions.push(
            "Load it now: sudo modprobe v4l2loopback video_nr=10,11 card_label=WaydroidCapture,GeneralCapture"
                .into(),
        );
    }

    // 2. Loopback nodes (reserved Waydroid node + a free general output)
    let devs = list_v4l2_devices(reserved);
    if devs.iter().any(|d| d.is_reserved) {
        notes.push(format!("The reserved Waydroid node ({reserved}) is present."));
    } else {
        issues.push(format!(
            "The reserved Waydroid capture node {reserved} is missing — the Waydroid-to-OBS pipeline will not stream."
        ));
        root_actions.push(
            "Load v4l2loopback so the Waydroid node exists (video_nr=10,...)".into(),
        );
    }
    if devs.iter().any(|d| d.is_output && !d.is_reserved) {
        notes.push("A free loopback output node is available for general capture.".into());
    } else {
        issues.push(
            "No free loopback output node is available for general capture (OBS would have nothing to read).".into(),
        );
        root_actions.push(
            "Load v4l2loopback with an extra node: sudo modprobe v4l2loopback video_nr=10,11".into(),
        );
    }

    // 3. Persistence across reboots (both nodes survive a restart)
    let conf = Path::new("/etc/modprobe.d/v4l2loopback.conf");
    let conf_body = std::fs::read_to_string(conf).unwrap_or_default();
    let persist_11 = conf_body.contains("video_nr=10,11") || conf_body.contains("video_nr=10, 11");
    if persist_11 && Path::new("/etc/modules-load.d/v4l2loopback.conf").exists() {
        notes.push("Both loopback nodes persist across reboots (modprobe + modules-load config).".into());
    } else {
        issues.push(
            "Only the Waydroid node persists across reboots — the general-capture output (e.g. /dev/video11) disappears after a restart.".into(),
        );
        root_actions.push(
            "Write 'options v4l2loopback video_nr=10,11 card_label=WaydroidCapture,GeneralCapture' to /etc/modprobe.d/v4l2loopback.conf and add v4l2loopback to /etc/modules-load.d/v4l2loopback.conf".into(),
        );
    }

    // 4. Desktop portal (PipeWire screencast for OBS)
    let portal_active = command::run_timeout(
        "systemctl",
        &["--user", "is-active", "xdg-desktop-portal.service"],
        5,
    )
    .map(|o| o.trim().eq_ignore_ascii_case("active"))
    .unwrap_or(false);
    if portal_active {
        match portal_source_types() {
            Some(t) if t > 0 => {
                notes.push(format!(
                    "The desktop portal is active and advertises {t} capture source type(s) — OBS lists Screen Capture (PipeWire)."
                ));
            }
            _ => {
                issues.push(
                    "The desktop portal is active but reports 0 screen-capture source types (stale portal state) — OBS hides its Screen Capture (PipeWire) source until the portal is restarted."
                        .into(),
                );
            }
        }
    } else {
        issues.push(
            "The desktop portal is not running — OBS hides its PipeWire screen-capture source until it is up.".into(),
        );
    }

    // 5. OBS + its PipeWire plugin (the option disappearing after reboot is
    //    usually a portal race, not a missing plugin — but verify the plugin).
    if command::which("obs") {
        let plugin = [
            "/usr/lib/x86_64-linux-gnu/obs-plugins/linux-pipewire.so",
            "/usr/lib/obs-plugins/linux-pipewire.so",
            "/usr/lib/obs-plugins/linux-pipewire.so.32",
            "/usr/lib/obs-plugins/64bit/linux-pipewire.so",
        ]
        .iter()
        .any(|p| Path::new(p).exists());
        if plugin {
            notes.push(
                "OBS Studio is installed with the linux-pipewire plugin (source: Screen Capture / Window Capture (PipeWire)).".into(),
            );
        } else {
            issues.push(
                "OBS Studio is installed but its linux-pipewire plugin is missing — the PipeWire screen-capture sources will never appear.".into(),
            );
        }
    } else {
        notes.push("OBS Studio is not installed; it will still be possible to capture to loopback nodes later.".into());
    }

    // 6. ffmpeg (camera / V4L2 / X11 capture workhorse)
    if command::which("ffmpeg") {
        notes.push("ffmpeg is installed (Camera / V4L2 / X11 capture).".into());
    } else {
        issues.push("ffmpeg is missing — Camera / V4L2 / X11 captures cannot start.".into());
        root_actions.push("Install ffmpeg: sudo apt install ffmpeg".into());
    }

    let d = CaptureDiagnosis {
        healthy: issues.is_empty(),
        issues,
        notes,
        root_actions,
    };
    log::debug!("capture diagnosis: healthy={} ({} issues)", d.healthy, d.issues.len());
    d
}

// ---- Per-source implementations ----

fn start_mobile(request: &CaptureRequest, reserved: &str) -> Result<GeneralCaptureStart> {
    if !command::which("scrcpy") {
        return Err(DremesError::NotFound("scrcpy".into()));
    }
    if !command::which("adb") {
        return Err(DremesError::NotFound("adb".into()));
    }

    let requested = request.source.trim();
    let devices = mobile_devices();
    let matched = if requested.is_empty() {
        devices.iter().find(|d| d.online)
    } else {
        devices.iter().find(|d| d.serial == requested || (d.serial.starts_with(&format!("{requested}:") )))
            .or_else(|| devices.iter().find(|d| d.online))
    };
    let Some(dev) = matched else {
        return Err(DremesError::Adb(
            "No online mobile device found. Connect an Android device over USB with USB debugging enabled, or use the Waydroid ADB endpoint.".into(),
        ));
    };
    let serial = dev.serial.clone();
    let out = resolve_output(request, reserved)?;

    let mut args: Vec<String> = vec![
        "--serial".into(),
        serial.clone(),
        "--no-window".into(),
        "--no-audio".into(),
        "--no-video-playback".into(),
        "--no-audio-playback".into(),
        "--v4l2-sink".into(),
        out.clone(),
    ];
    if request.fps > 0 {
        args.push("--max-fps".into());
        args.push(request.fps.to_string());
    }
    let handle = spawn("scrcpy", &args)?;
    Ok(GeneralCaptureStart {
        handle: Box::new(handle),
        output_device: out,
        method: "scrcpy".into(),
        connection: serial.clone(),
        source: serial,
        width: 0,
        height: 0,
        fps: request.fps,
    })
}

fn start_v4l2_source(
    request: &CaptureRequest,
    reserved: &str,
    _session: &SessionInfo,
) -> Result<GeneralCaptureStart> {
    if !command::which("ffmpeg") {
        return Err(DremesError::NotFound("ffmpeg".into()));
    }
    let source = request.source.trim();
    let from = if !source.is_empty() {
        source.to_string()
    } else if request.kind == CaptureSourceKind::V4l2Device {
        return Err(DremesError::msg("Select a V4L2 device to capture."));
    } else if let Some(cam) = first_camera_source() {
        cam
    } else {
        return Err(DremesError::V4l2(
            "No camera / V4L2 source found to capture.".into(),
        ));
    };
    if !v4l2::device_exists(&from) {
        return Err(DremesError::V4l2(format!("source device {from} does not exist.")));
    }
    let out = resolve_output(request, reserved)?;

    // Safety: never use a loopback/output node as a capture source, and never
    // feed a device into itself (that spawns ffmpeg reading a write-only node,
    // which errors instantly or spins forever).
    let loops: Vec<String> = list_v4l2_devices(reserved)
        .into_iter()
        .filter(|d| d.is_loopback)
        .map(|d| d.path)
        .collect();
    if loops.contains(&from) {
        let suggestion = first_camera_source().unwrap_or_default();
        let hint = if suggestion.is_empty() {
            String::new()
        } else {
            format!(" Pick a camera or real V4L2 input instead (e.g. {suggestion}).")
        };
        return Err(DremesError::Capture(format!(
            "{from} is a loopback output device, not a capture source.{hint}"
        )));
    }
    if from == out {
        return Err(DremesError::Capture(format!(
            "Source and output are the same device ({from}); that would capture from its own output. Pick a different source."
        )));
    }

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "v4l2".into(),
    ];
    if request.fps > 0 {
        args.push("-framerate".into());
        args.push(request.fps.to_string());
    }
    if request.width > 0 && request.height > 0 {
        args.push("-video_size".into());
        args.push(format!("{}x{}", request.width, request.height));
    }
    args.push("-i".into());
    args.push(from.clone());
    args.push("-f".into());
    args.push("v4l2".into());
    args.push("-pix_fmt".into());
    args.push("yuyv422".into());
    if request.fps > 0 {
        args.push("-r".into());
        args.push(request.fps.to_string());
    }
    args.push(out.clone());

    let handle = spawn("ffmpeg", &args)?;
    Ok(GeneralCaptureStart {
        handle: Box::new(handle),
        output_device: out,
        method: "ffmpeg-v4l2".into(),
        connection: from.clone(),
        source: from,
        width: request.width,
        height: request.height,
        fps: request.fps,
    })
}

fn start_monitor(request: &CaptureRequest, reserved: &str, session: &SessionInfo) -> Result<GeneralCaptureStart> {
    let out = resolve_output(request, reserved)?;
    if session.wayland {
        if !command::which("wf-recorder") {
            return Err(DremesError::msg(
                "Monitor capture under Wayland requires wf-recorder (WLROOTS/LabWC compositors). Install wf-recorder, or use OBS Studio Screen Capture on GNOME/XFCE/KDE.",
            ));
        }
        let args: Vec<String> = vec![
            "--muxer=v4l2".into(),
            format!("--file={out}"),
        ];
        let handle = spawn("wf-recorder", &args)?;
        return Ok(GeneralCaptureStart {
            handle: Box::new(handle),
            output_device: out,
            method: "wf-recorder".into(),
            connection: "Wayland (presentation buffer)".into(),
            source: String::new(),
            width: 0,
            height: 0,
            fps: request.fps,
        });
    }

    if !command::which("ffmpeg") {
        return Err(DremesError::NotFound("ffmpeg".into()));
    }
    let display = if !session.display.is_empty() {
        session.display.clone()
    } else {
        ":0".into()
    };
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "x11grab".into(),
    ];
    if request.width > 0 && request.height > 0 {
        args.push("-video_size".into());
        args.push(format!("{}x{}", request.width, request.height));
    }
    args.push("-i".into());
    args.push(display.clone());
    args.push("-f".into());
    args.push("v4l2".into());
    args.push("-pix_fmt".into());
    args.push("yuyv422".into());
    if request.fps > 0 {
        args.push("-r".into());
        args.push(request.fps.to_string());
    }
    args.push(out.clone());

    let handle = spawn("ffmpeg", &args)?;
    Ok(GeneralCaptureStart {
        handle: Box::new(handle),
        output_device: out,
        method: "ffmpeg-x11grab".into(),
        connection: display.clone(),
        source: String::new(),
        width: request.width,
        height: request.height,
        fps: request.fps,
    })
}

fn start_window(request: &CaptureRequest, reserved: &str, session: &SessionInfo) -> Result<GeneralCaptureStart> {
    if session.wayland {
        return Err(DremesError::msg(
            "Native window capture under Wayland is provided by OBS Studio's Window Capture source. Dremes cannot grab a specific window on Wayland directly.",
        ));
    }
    if !command::which("ffmpeg") {
        return Err(DremesError::NotFound("ffmpeg".into()));
    }
    if !command::which("xdotool") {
        return Err(DremesError::NotFound("xdotool".into()));
    }

    // Resolve the target window: explicit id/title, else the active window.
    let target = if !request.source.trim().is_empty() {
        request.source.trim().to_string()
    } else {
        command::run("xdotool", &["getactivewindow"])?
            .trim()
            .to_string()
    };
    let (x, y, w, h) = window_geometry(&target)?;
    let out = resolve_output(request, reserved)?;
    let display = if !session.display.is_empty() {
        session.display.clone()
    } else {
        ":0".into()
    };

    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "x11grab".into(),
        "-video_size".into(),
        format!("{w}x{h}"),
        "-i".into(),
        format!("{display}+{x},{y}"),
        "-f".into(),
        "v4l2".into(),
        "-pix_fmt".into(),
        "yuyv422".into(),
    ];
    if request.fps > 0 {
        args.push("-r".into());
        args.push(request.fps.to_string());
    }
    args.push(out.clone());

    let handle = spawn("ffmpeg", &args)?;
    Ok(GeneralCaptureStart {
        handle: Box::new(handle),
        output_device: out,
        method: "ffmpeg-x11grab-window".into(),
        connection: target.clone(),
        source: target,
        width: w,
        height: h,
        fps: request.fps,
    })
}

fn game_guidance() -> DremesError {
    if has_vkcapture_layer() {
        DremesError::msg(
            "Game capture uses OBS Studio with the OBS-VKCapture Vulkan layer: start OBS, add a Game Capture source, then launch the game with `obs-vkcapture <command>` so its frames stream to that source. Dremes does not launch games itself.",
        )
    } else {
        DremesError::msg(
            "Game capture needs the optional OBS-VKCapture backend (obs-vkcapture) plus OBS Studio. Install obs-vkcapture, launch the game under OBS with a Game Capture source.",
        )
    }
}

/// Number of screen-capture source types the desktop portal advertises
/// (1=monitor, 2=window, 4=monitor+audio, ...), or None if unknown/unreadable.
///
/// A portal that is *active* but returns 0 here is stale: it registered before
/// its backend was ready, so OBS sees "No capture sources available" and hides
/// the Screen Capture (PipeWire) source until the portal is restarted.
pub(crate) fn portal_source_types() -> Option<u32> {
    if command::which("busctl") {
        if let Ok(out) = command::run_timeout(
            "busctl",
            &[
                "--user",
                "get-property",
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.ScreenCast",
                "AvailableSourceTypes",
            ],
            5,
        ) {
            if let Some(t) = parse_uint(out.trim()) {
                return Some(t);
            }
        }
    }
    // Fallback for hosts without busctl: parse gdbus GetAll output.
    if let Ok(out) = command::run_timeout(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.DBus.Properties.GetAll",
            "org.freedesktop.portal.ScreenCast",
        ],
        5,
    ) {
        let s = out;
        if let Some(start) = s.find("AvailableSourceTypes") {
            let end = s[start..]
                .find("AvailableCursorModes")
                .map(|e| start + e)
                .unwrap_or(s.len());
            return parse_uint(&s[start..end]);
        }
    }
    None
}

/// Parse the portal's capture-source-type value from either
/// `busctl` output ("u 7") or a `gdbus` slice ("...'AvailableSourceTypes': <uint32 7> ...").
fn parse_uint(s: &str) -> Option<u32> {
    if let Some((_, after)) = s.split_once("<uint32") {
        let digits: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return digits.parse().ok();
    }
    // busctl style: take the last contiguous digit run.
    let mut last: Option<&str> = None;
    let mut run_start: Option<usize> = None;
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_digit() {
            run_start.get_or_insert(i);
        } else if let Some(st) = run_start.take() {
            last = Some(&s[st..i]);
        }
    }
    last.or_else(|| run_start.map(|st| &s[st..]))
        .and_then(|d| d.parse().ok())
}

/// Whether the OBS-VKCapture Vulkan implicit layer is installed.
fn has_vkcapture_layer() -> bool {
    const DIRS: [&str; 3] = [
        "/usr/share/vulkan/implicit_layer.d",
        "/usr/local/share/vulkan/implicit_layer.d",
        "/etc/vulkan/implicit_layer.d",
    ];
    DIRS.iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten().any(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.contains("obs") || name.contains("vkcapture")
                })
            })
            .unwrap_or(false)
    })
}

/// Read the geometry of an X11 window via xdotool.
fn window_geometry(target: &str) -> Result<(i32, i32, u32, u32)> {
    let out = command::run("xdotool", &["getwindowgeometry", "--shell", target])?;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut w = 0u32;
    let mut h = 0u32;
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, val) = line.split_once('=').unwrap_or((line, ""));
        if let Ok(v) = val.trim().parse::<i32>() {
            match key {
                "X" => x = v,
                "Y" => y = v,
                "WIDTH" => w = v.max(0) as u32,
                "HEIGHT" => h = v.max(0) as u32,
                _ => {}
            }
        }
    }
    if w == 0 || h == 0 {
        return Err(DremesError::msg(format!(
            "Could not determine geometry for window {target}."
        )));
    }
    Ok((x, y, w, h))
}

// ---- Process spawning + ownership ----

/// A capture handle that knows both the pid and the binary that produced it, so
/// a recycled pid is never signaled. It also owns the `Child` it spawned so a
/// dead writer is reaped (never left as a zombie) and `is_alive` reflects true
/// liveness plus a crash that `kill -0` would miss.
pub struct PidHandle {
    pid: u32,
    binary: &'static str,
    child: std::sync::Mutex<Option<std::process::Child>>,
}

impl PidHandle {
    #[cfg(test)]
    pub fn for_test(pid: u32, binary: &'static str) -> Self {
        Self {
            pid,
            binary,
            child: std::sync::Mutex::new(None),
        }
    }
}

impl GeneralCaptureHandle for PidHandle {
    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn is_alive(&self) -> bool {
        let mut guard = self.child.lock().unwrap();
        match guard.as_mut() {
            // try_wait both reaps dead children and reports zombie-as-dead.
            Some(c) => c.try_wait().map(|st| st.is_none()).unwrap_or(true),
            None => signal(self.pid, "0"),
        }
    }

    fn stop(&self) -> Result<()> {
        if !cmdline_matches(self.pid, self.binary) {
            log::warn!(
                "Refusing to signal PID {} as a general-capture process: command line no longer matches {}.",
                self.pid,
                self.binary
            );
            return Ok(());
        }
        let mut guard = self.child.lock().unwrap();
        // Already exited (including zombie state): reap and be done.
        if let Some(c) = guard.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                return Ok(());
            }
        }
        // Graceful SIGTERM, then SIGKILL fallback. Reap via try_wait so no
        // zombie lingers under the Engine (its parent).
        let _ = signal(self.pid, "TERM");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        while std::time::Instant::now() < deadline {
            if let Some(c) = guard.as_mut() {
                if let Ok(Some(_)) = c.try_wait() {
                    return Ok(());
                }
            } else if !signal(self.pid, "0") {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = signal(self.pid, "KILL");
        let _ = guard.as_mut().and_then(|c| c.try_wait().ok());
        Ok(())
    }
}

/// Spawn a long-lived capture process detached from the controlling terminal.
/// The returned handle owns the `Child`, letting the Engine reap it on exit.
fn spawn(binary: &'static str, args: &[String]) -> Result<PidHandle> {
    let child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DremesError::NotFound(binary.into())
            } else {
                DremesError::Capture(format!("failed to start {binary}: {e}"))
            }
        })?;
    let pid = child.id();
    Ok(PidHandle {
        pid,
        binary,
        child: std::sync::Mutex::new(Some(child)),
    })
}

/// Send a signal to a pid via `kill`. Returns Ok(true) when the signal went
/// through (or, for signal 0, the process exists).
fn signal(pid: u32, sig: &str) -> bool {
    Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whether the process's current command line still names the expected binary.
/// Blocks signaling a recycled pid that is now some unrelated process.
fn cmdline_matches(pid: u32, binary: &str) -> bool {
    let cl = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| s.replace('\0', " "))
        .unwrap_or_default();
    if cl.is_empty() {
        return false;
    }
    cl.trim().starts_with(binary) || cl.contains(binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mobile_devices_rows() {
        let out = "List of devices attached\nR58N123456\tdevice product:waydroid model:Waydroid transport_id:1\n192.168.240.112:5555 device product:waydroid transport_id:2\nOFFLINE123 unauthorized\n";
        let devs = parse_mobile_devices(out);
        assert_eq!(devs.len(), 3);
        assert!(devs[0].online);
        assert_eq!(devs[0].product, "waydroid");
        assert_eq!(devs[0].model, "Waydroid");
        assert_eq!(devs[0].transport, "1");
        assert_eq!(devs[1].serial, "192.168.240.112:5555");
        assert!(!devs[2].online);
    }

    #[test]
    fn parses_mobile_devices_empty() {
        assert!(parse_mobile_devices("List of devices attached\n").is_empty());
        assert!(parse_mobile_devices("").is_empty());
    }

    #[test]
    fn parses_portal_source_type_outputs() {
        assert_eq!(parse_uint("u 7"), Some(7));
        assert_eq!(parse_uint("u 0"), Some(0));
        assert_eq!(parse_uint("<uint32 7>"), Some(7));
        assert_eq!(parse_uint("<uint32 0>"), Some(0));
        let gdbus_slice = "({'AvailableSourceTypes': <uint32 7>, 'AvailableCursorModes': ";
        assert_eq!(parse_uint(gdbus_slice), Some(7));
        let gdbus_zero = "({'AvailableSourceTypes': <uint32 0>, 'AvailableCursorModes': <uint32 7>";
        assert_eq!(parse_uint(gdbus_zero), Some(0));
        assert_eq!(parse_uint("garbage"), None);
    }

    #[test]
    fn reserved_device_is_never_an_output() {
        let request = CaptureRequest {
            kind: CaptureSourceKind::Monitor,
            source: String::new(),
            output_device: "/dev/video10".into(),
            width: 0,
            height: 0,
            fps: 0,
        };
        assert!(start(&request, "/dev/video10").is_err());
    }

    #[test]
    fn resolve_output_rejects_reserved() {
        let req = CaptureRequest {
            kind: CaptureSourceKind::Monitor,
            source: String::new(),
            output_device: "/dev/video10".into(),
            width: 0,
            height: 0,
            fps: 0,
        };
        assert!(resolve_output(&req, "/dev/video10").is_err());
        let req2 = CaptureRequest {
            output_device: "/dev/video11".into(),
            ..CaptureRequest::empty(CaptureSourceKind::Monitor)
        };
        // /dev/video11 may not exist in CI: any clear error is acceptable — the
        // reserved check itself is what we test above.
        let _ = resolve_output(&req2, "/dev/video10");
    }

    #[test]
    fn cmdline_guard_refuses_unrelated() {
        // pid 1 (init) never matches "scrcpy"/"ffmpeg"/"wf-recorder".
        assert!(!cmdline_matches(1, "scrcpy"));
        assert!(!cmdline_matches(1, "ffmpeg"));
        assert!(!cmdline_matches(1, "wf-recorder"));
    }
}