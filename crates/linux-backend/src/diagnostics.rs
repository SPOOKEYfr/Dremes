//! System diagnostics for the Diagnostics page and first-run experience.
//!
//! Checks availability/version of the components Dremes uses. Each check
//! reports PASS / WARNING / FAIL with an understandable explanation.

use dremes_core::platform::DiagnosticItem;

use crate::command;
use crate::v4l2;

/// Run all diagnostics. Never panics; individual failures become FAIL items.
pub fn run() -> Vec<DiagnosticItem> {
    let mut items: Vec<DiagnosticItem> = Vec::new();

    items.push(check_exec("waydroid", "Waydroid"));
    items.push(waydroid_version());
    items.push(check_exec("adb", "ADB"));
    items.push(check_exec("scrcpy", "scrcpy"));
    items.push(check_exec("ffmpeg", "FFmpeg (optional)"));
    items.push(v4l2loopback());
    items.push(v4l2_device());
    items.push(v4l2_permissions());
    items.push(obs());
    items.push(architecture());
    items.push(display_resolution());
    items.push(session());
    items.push(pipewire());
    items.push(kernel());

    items
}

fn check_exec(exec: &str, label: &str) -> DiagnosticItem {
    let found = command::which(exec);
    DiagnosticItem {
        key: exec.to_string(),
        label: label.into(),
        status: if found { "pass" } else { "fail" },
        message: if found {
            format!("Installed ({})", exec)
        } else {
            format!("Not found on PATH. Install `{exec}`.")
        },
    }
}

fn waydroid_version() -> DiagnosticItem {
    match command::run("waydroid", &["--version"]) {
        Ok(v) => {
            let first = v.lines().next().unwrap_or("").trim().to_string();
            DiagnosticItem {
                key: "waydroid-version".into(),
                label: "Waydroid version".into(),
                status: "pass",
                message: if first.is_empty() { "Installed".into() } else {
                    format!("Waydroid {first}")
                },
            }
        }
        Err(e) => DiagnosticItem {
            key: "waydroid-version".into(),
            label: "Waydroid version".into(),
            status: "warning",
            message: format!("Could not determine version: {e}"),
        },
    }
}

fn v4l2loopback() -> DiagnosticItem {
    let module_loaded = command::run_timeout("grep", &["-c", "^v4l2loopback", "/proc/modules"], 3)
        .map(|o| o.trim().parse::<u32>().unwrap_or(0) > 0)
        .unwrap_or(false);
    let ctl = command::which("v4l2loopback-ctl");
    DiagnosticItem {
        key: "v4l2loopback".into(),
        label: "v4l2loopback".into(),
        status: if module_loaded || ctl { "pass" } else { "warning" },
        message: match (module_loaded, ctl) {
            (true, _) => "Module loaded, v4l2loopback available".into(),
            (false, true) => "v4l2loopback tools present but module not loaded".into(),
            (false, false) => "v4l2loopback not detected".into(),
        },
    }
}

fn v4l2_device() -> DiagnosticItem {
    let device = crate::v4l2::DEFAULT_DEVICE;
    if v4l2::device_exists(device) {
        DiagnosticItem {
            key: "v4l2-device".into(),
            label: "/dev/video10".into(),
            status: "pass",
            message: format!("Exists ({device})"),
        }
    } else {
        DiagnosticItem {
            key: "v4l2-device".into(),
            label: "/dev/video10".into(),
            status: "warning",
            message: format!("{device} not present; will be created when v4l2loopback loads"),
        }
    }
}

fn v4l2_permissions() -> DiagnosticItem {
    let device = crate::v4l2::DEFAULT_DEVICE;
    if !v4l2::device_exists(device) {
        return DiagnosticItem {
            key: "v4l2-perms".into(),
            label: "V4L2 permissions".into(),
            status: "warning",
            message: "Deferring permission check (device absent)".into(),
        };
    }
    match std::fs::OpenOptions::new().read(true).open(device) {
        Ok(_) => DiagnosticItem {
            key: "v4l2-perms".into(),
            label: "V4L2 permissions".into(),
            status: "pass",
            message: "Read access OK".into(),
        },
        Err(e) => DiagnosticItem {
            key: "v4l2-perms".into(),
            label: "V4L2 permissions".into(),
            status: "fail",
            message: format!("Cannot open {device}: {e}"),
        },
    }
}

fn obs() -> DiagnosticItem {
    let found = command::which("obs") || command::which("obs-studio");
    DiagnosticItem {
        key: "obs".into(),
        label: "OBS Studio".into(),
        status: if found { "pass" } else { "warning" },
        message: if found {
            "Installed".into()
        } else {
            "OBS not detected (optional for capture, required for streaming to OBS)".into()
        },
    }
}

fn architecture() -> DiagnosticItem {
    let arch = std::env::consts::ARCH;
    DiagnosticItem {
        key: "arch".into(),
        label: "System architecture".into(),
        status: "pass",
        message: format!("{arch}"),
    }
}

fn display_resolution() -> DiagnosticItem {
    // Try xrandr, else gsettings/fallback.
    if let Ok(o) = command::run_timeout("xrandr", &[], 3) {
        if let Some(line) = o.lines().find(|l| l.contains('*')) {
            let res = line.split_whitespace().next().unwrap_or("").to_string();
            return DiagnosticItem {
                key: "resolution".into(),
                label: "Display resolution".into(),
                status: "pass",
                message: res,
            };
        }
    }
    DiagnosticItem {
        key: "resolution".into(),
        label: "Display resolution".into(),
        status: "warning",
        message: "Could not detect via xrandr (Wayland)".into(),
    }
}

fn session() -> DiagnosticItem {
    let wayland = std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false);
    let x11 = std::env::var("DISPLAY").is_ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    DiagnosticItem {
        key: "session".into(),
        label: "Display session".into(),
        status: "pass",
        message: if wayland || x11 {
            format!("{}/{} (direct Waydroid capture does not need screen capture)", session_type, if wayland { "Wayland" } else { "X11" })
        } else {
            format!("Unknown session type: {session_type}")
        },
    }
}

fn pipewire() -> DiagnosticItem {
    // PipeWire is NOT a dependency for the direct capture path.
    if command::which("pipewire") {
        DiagnosticItem {
            key: "pipewire".into(),
            label: "PipeWire".into(),
            status: "pass",
            message: "Present (informational; not required for capture)".into(),
        }
    } else {
        DiagnosticItem {
            key: "pipewire".into(),
            label: "PipeWire".into(),
            status: "pass",
            message: "Not detected (not required for direct capture)".into(),
        }
    }
}

fn kernel() -> DiagnosticItem {
    let mut k = String::new();
    if let Ok(o) = std::fs::read_to_string("/proc/sys/kernel/osrelease").or_else(|_| std::fs::read_to_string("/proc/version")) {
        k = o.trim().split_whitespace().next().unwrap_or("").to_string();
    }
    if k.is_empty() {
        k = std::env::consts::OS.to_string();
    }
    DiagnosticItem {
        key: "kernel".into(),
        label: "Kernel".into(),
        status: "pass",
        message: k,
    }
}
