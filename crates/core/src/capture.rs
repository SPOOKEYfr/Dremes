//! General-purpose capture model.
//!
//! Beyond the dedicated Waydroid → scrcpy → /dev/video10 pipeline, Dremes can
//! record *any* source (a USB/ADB mobile device, a native window, the whole
//! monitor, a camera, or an arbitrary V4L2 device) into a free loopback V4L2
//! device that OBS (or any V4L2 consumer) can read — while never touching the
//! Waydroid capture device (e.g. `/dev/video10`), which stays exclusively
//! owned by the Waydroid pipeline.
//!
//! Every requested capture becomes a [`CaptureRequest`]. The platform backend
//! is the single authority that turns a request into a concrete method, a
//! source connection and a writable output device, and reports an honest
//! [`CaptureBackendStatus`] so the GUI can show "backend unavailable" instead
//! of "capture broken". Everything in this module is a pure data type or pure
//! function — no shell, no I/O — so the detection logic is shared, testable,
//! and identical between the Engine and the backends.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A handle to a live general-purpose capture writer process.
///
/// Unlike the Waydroid pipeline's [`crate::platform::CaptureHandle`], a general
/// capture may be any producer (scrcpy, ffmpeg, wf-recorder...). The concrete
/// handle knows the spawned binary so it can refuse to terminate a recycled
/// PID that no longer matches.
pub trait GeneralCaptureHandle: Send + Sync {
    /// The OS pid of the writer, if known.
    fn pid(&self) -> Option<u32>;
    /// Whether the writer process is still alive.
    fn is_alive(&self) -> bool;
    /// Gracefully stop the writer (SIGTERM then SIGKILL fallback).
    fn stop(&self) -> Result<()>;
}

/// The types of capture source the user can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureSourceKind {
    /// A physical/emulated Android device over USB/ADB.
    Mobile,
    /// A native game (Vulkan/OpenGL) via an explicit capture layer.
    Game,
    /// A single native window.
    Window,
    /// Whole-screen capture (all monitors).
    Monitor,
    /// A built-in/USB camera exposed as a V4L2 device.
    Camera,
    /// An arbitrary V4L2 device the user selected explicitly.
    V4l2Device,
}

impl CaptureSourceKind {
    /// A stable string identifier for the source kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            CaptureSourceKind::Mobile => "Mobile",
            CaptureSourceKind::Game => "Game",
            CaptureSourceKind::Window => "Window",
            CaptureSourceKind::Monitor => "Monitor",
            CaptureSourceKind::Camera => "Camera",
            CaptureSourceKind::V4l2Device => "V4L2 Device",
        }
    }
}

/// A user request to start a general capture.
///
/// `source` is context-dependent (e.g. a mobile serial, a window id/title, a
/// V4L2 video node). `output_device` may be empty, in which case the backend
/// picks the first free loopback device. `width`/`height`/`fps` of zero mean
/// "auto / source default".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRequest {
    pub kind: CaptureSourceKind,
    /// Source reference: serial for Mobile, window id/title for Window,
    /// video node for Camera/V4l2Device. Empty for Monitor or auto.
    pub source: String,
    /// The output loopback device, or empty for backend auto-selection.
    pub output_device: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl CaptureRequest {
    /// An "empty" request for a source kind (all-zero, auto-select everything).
    pub fn empty(kind: CaptureSourceKind) -> Self {
        Self {
            kind,
            source: String::new(),
            output_device: String::new(),
            width: 0,
            height: 0,
            fps: 0,
        }
    }
}

/// A detected V4L2 video device on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4l2DeviceInfo {
    /// The device node (e.g. `/dev/video10`).
    pub path: String,
    /// The friendly name read from sysfs (e.g. "WaydroidCapture").
    pub name: String,
    /// True when the device is a v4l2loopback (output + capture) device.
    pub is_loopback: bool,
    /// True when the device can be read as a capture source.
    pub is_capture: bool,
    /// True when the device exposes an output side a producer can write to.
    pub is_output: bool,
    /// True when this device is reserved for the Waydroid capture pipeline and
    /// must never be used by a general capture.
    pub is_reserved: bool,
}

/// A mobile device as reported by `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileDeviceInfo {
    pub serial: String,
    /// "device" | "offline" | "unauthorized" | ...
    pub state: String,
    pub product: String,
    pub model: String,
    pub transport: String,
    /// True when the device is online and ready to capture.
    pub online: bool,
}

/// Detected desktop/session information.
///
/// Reported faithfully from environment variables only — never guessed from a
/// frozen distribution list. "unknown"/"other" are honest values Dremes shows
/// as-is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// "wayland" | "x11" | "unknown".
    pub session_type: String,
    /// The normalized desktop environment id (e.g. "gnome", "cinnamon").
    pub desktop: String,
    /// The family grouping: gnome | cinnamon | xfce | kde | other | unknown.
    pub desktop_family: String,
    /// True when the session is (or reports) a Wayland session.
    pub wayland: bool,
    /// The DISPLAY value (e.g. ":0"). Empty when unset.
    pub display: String,
    /// The WAYLAND_DISPLAY value. Empty when unset.
    pub wayland_display: String,
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self {
            session_type: "unknown".into(),
            desktop: "unknown".into(),
            desktop_family: "unknown".into(),
            wayland: false,
            display: String::new(),
            wayland_display: String::new(),
        }
    }
}

impl SessionInfo {
    /// Detect the session purely from the ambient environment values. The
    /// signature takes raw optionals so detection can be unit-tested without a
    /// real display server.
    pub fn detect(
        xdg_session_type: Option<&str>,
        xdg_current_desktop: Option<&str>,
        wayland_display: Option<&str>,
        display: Option<&str>,
    ) -> Self {
        let session_type = xdg_session_type
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let wl_display = wayland_display.filter(|d| !d.trim().is_empty()).unwrap_or("").to_string();
        let x_display = display.filter(|d| !d.trim().is_empty()).unwrap_or("").to_string();

        // Decide the display model: prefer XDG_SESSION_TYPE, then fall back to
        // the presence of WAYLAND_DISPLAY / DISPLAY.
        let wayland = match session_type.as_str() {
            "wayland" => true,
            "x11" => false,
            _ => !wl_display.is_empty(),
        };

        let (desktop, desktop_family) = normalize_desktop(xdg_current_desktop.unwrap_or(""));

        Self {
            session_type,
            desktop,
            desktop_family,
            wayland,
            display: x_display,
            wayland_display: wl_display,
        }
    }
}

/// The fixed set of desktop identifiers we recognize from XDG_CURRENT_DESKTOP.
fn desktop_family_of(name: &str) -> Option<&'static str> {
    match name {
        "gnome" | "mutter" | "ubuntu" | "pop" | "pantheon" => Some("gnome"),
        "cinnamon" => Some("cinnamon"),
        "xfce" => Some("xfce"),
        "kde" | "plasma" => Some("kde"),
        "budgie" => Some("gnome"),
        "mate" => Some("other"),
        "lxqt" | "lxe" | "sway" | "i3" | "hyprland" | "river" | "wayfire" => Some("other"),
        _ => None,
    }
}

/// Whether the token is a distinct desktop environment identifier (as opposed
/// to a distribution/session flavor like "ubuntu").
fn is_primary_desktop(name: &str) -> bool {
    matches!(name, "gnome" | "cinnamon" | "xfce" | "kde" | "plasma" | "mate" | "budgie" | "lxqt")
}

/// Normalize a raw `XDG_CURRENT_DESKTOP` value into `(desktop, family)`.
///
/// `XDG_CURRENT_DESKTOP` is a colon-separated list (e.g. "ubuntu:GNOME"); the
/// distinct desktop identifier wins over flavor tokens, so "ubuntu:GNOME"
/// resolves to desktop "gnome". When nothing matches we report the first token
/// and family "other" — honestly, never fabricated.
fn normalize_desktop(raw: &str) -> (String, String) {
    let tokens: Vec<String> = raw
        .split(':')
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return ("unknown".into(), "unknown".into());
    }
    // Prefer a distinct desktop identifier token.
    for t in &tokens {
        if is_primary_desktop(t) {
            return (t.clone(), desktop_family_of(t).unwrap_or("other").into());
        }
    }
    // Fall back to the first token that maps to a known family.
    for t in &tokens {
        if let Some(family) = desktop_family_of(t) {
            return (t.clone(), family.into());
        }
    }
    (tokens[0].clone(), "other".into())
}

/// Honest status of the capture-producing backends on this host.
///
/// `available` is true when at least one capture method can start. `notes`
/// records which optional tools are present/missing so the UI can explain
/// exactly why a source is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CaptureBackendStatus {
    pub available: bool,
    pub notes: Vec<String>,
}

/// What the user can capture right now, discovered on demand.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureCatalog {
    pub session: SessionInfo,
    pub backend_status: CaptureBackendStatus,
    /// Output devices Dremes may write to (excluding the reserved Waydroid
    /// device).
    pub output_devices: Vec<V4l2DeviceInfo>,
    /// All V4L2 sources (cameras and arbitrary devices).
    pub source_devices: Vec<V4l2DeviceInfo>,
    /// Mobile (ADB) devices currently visible.
    pub mobile_devices: Vec<MobileDeviceInfo>,
}

impl CaptureCatalog {
    /// The first free (non-reserved) loopback output device, if any.
    pub fn first_output_device(&self) -> Option<&V4l2DeviceInfo> {
        self.output_devices.first()
    }
}

/// Result of the capture health check shown by the GUI's "Fix All" panel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureDiagnosis {
    /// True when no issues were found (issues may still be shown as notes).
    pub healthy: bool,
    /// Problems that need attention (shown as errors in the GUI).
    pub issues: Vec<String>,
    /// Reassuring facts about the current setup.
    pub notes: Vec<String>,
    /// Actions that require root privileges (password prompt from the GUI).
    pub root_actions: Vec<String>,
}

impl CaptureDiagnosis {
    pub fn healthy_default() -> Self {
        Self {
            healthy: true,
            issues: Vec::new(),
            notes: Vec::new(),
            root_actions: Vec::new(),
        }
    }
}

/// Lifecycle state of a general capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptureRunState {
    #[default]
    Idle,
    Starting,
    Active,
    Stopping,
    Error,
}

impl CaptureRunState {
    pub fn is_active(&self) -> bool {
        matches!(self, CaptureRunState::Starting | CaptureRunState::Active | CaptureRunState::Stopping)
    }
}

/// The live report of a general capture session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureSessionInfo {
    pub state: CaptureRunState,
    pub kind: Option<CaptureSourceKind>,
    /// The concrete source reference (serial / window / video node).
    pub source: String,
    /// The loopback device the capture writes to.
    pub output_device: String,
    /// The capture method in use (e.g. "scrcpy", "ffmpeg-x11grab").
    pub method: String,
    /// The connection detail (e.g. display string or ADB endpoint).
    pub connection: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Unix-epoch seconds the session started (0 when idle).
    pub started_at_unix: u64,
    /// Last-known human-readable error (empty when healthy).
    pub error: String,
}

/// True when `device` is the reserved Waydroid capture node and must never be
/// used as a general-capture output.
pub fn is_reserved_device(device: &str, reserved: &str) -> bool {
    let d = device.trim();
    let r = reserved.trim();
    !d.is_empty() && !r.is_empty() && d == r
}

/// Ensure nobody passes the Waydroid capture device to a general capture.
///
/// The Engine enforces this before forwarding any request to the backend.
pub fn ensure_not_reserved(request: &CaptureRequest, reserved: &str) -> Result<()> {
    if is_reserved_device(&request.output_device, reserved) {
        return Err(crate::error::DremesError::Capture(format!(
            "{} is reserved for the Waydroid capture pipeline and cannot be used by general capture.",
            request.output_device
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_kind_has_stable_labels() {
        assert_eq!(CaptureSourceKind::Mobile.as_str(), "Mobile");
        assert_eq!(CaptureSourceKind::V4l2Device.as_str(), "V4L2 Device");
        assert_ne!(CaptureSourceKind::Game, CaptureSourceKind::Mobile);
    }

    #[test]
    fn empty_request_has_zeros() {
        let r = CaptureRequest::empty(CaptureSourceKind::Monitor);
        assert_eq!(r.width, 0);
        assert_eq!(r.fps, 0);
        assert!(r.output_device.is_empty());
        assert!(r.source.is_empty());
    }

    #[test]
    fn reserved_device_guard_blocks_video10() {
        let mut r = CaptureRequest::empty(CaptureSourceKind::Monitor);
        r.output_device = "/dev/video10".into();
        assert!(ensure_not_reserved(&r, "/dev/video10").is_err());
        r.output_device = "/dev/video11".into();
        assert!(ensure_not_reserved(&r, "/dev/video10").is_ok());
        r.output_device.clear();
        assert!(ensure_not_reserved(&r, "/dev/video10").is_ok());
    }

    #[test]
    fn is_reserved_device_handles_blanks() {
        assert!(!is_reserved_device("", "/dev/video10"));
        assert!(!is_reserved_device(" ", ""));
        assert!(!is_reserved_device("/dev/video11", "/dev/video10"));
        assert!(is_reserved_device("/dev/video10", "/dev/video10"));
    }

    #[test]
    fn session_detect_wayland_gnome() {
        let s = SessionInfo::detect(Some("wayland"), Some("ubuntu:GNOME"), Some("wayland-0"), Some(":0"));
        assert!(s.wayland);
        assert_eq!(s.session_type, "wayland");
        assert_eq!(s.desktop, "gnome");
        assert_eq!(s.desktop_family, "gnome");
        assert_eq!(s.display, ":0");
        assert_eq!(s.wayland_display, "wayland-0");
    }

    #[test]
    fn session_detect_x11_xfce() {
        let s = SessionInfo::detect(Some("x11"), Some("XFCE"), None, Some(":1"));
        assert!(!s.wayland);
        assert_eq!(s.desktop, "xfce");
        assert_eq!(s.desktop_family, "xfce");
    }

    #[test]
    fn session_detect_unknown_is_honest() {
        let s = SessionInfo::detect(None, Some("AMAZING-SHELL"), None, Some(":0"));
        assert_eq!(s.desktop_family, "other");
        assert_eq!(s.desktop, "amazing-shell");
        let s2 = SessionInfo::detect(None, None, None, None);
        assert_eq!(s2.desktop_family, "unknown");
        assert_eq!(s2.session_type, "unknown");
        // Same unknown session on a Wayland-by-default compositor when only
        // WAYLAND_DISPLAY is set.
        let s3 = SessionInfo::detect(None, None, Some("wayland-1"), None);
        assert!(s3.wayland);
    }

    #[test]
    fn captured_request_roundtrips_json() {
        let req = CaptureRequest {
            kind: CaptureSourceKind::Mobile,
            source: "R58N123456".into(),
            output_device: "/dev/video11".into(),
            width: 1920,
            height: 1080,
            fps: 60,
        };
        let back: CaptureRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn first_output_skips_reserved() {
        let catalog = CaptureCatalog {
            output_devices: vec![
                V4l2DeviceInfo {
                    path: "/dev/video11".into(),
                    name: "Dremes Capture".into(),
                    is_loopback: true,
                    is_capture: true,
                    is_output: true,
                    is_reserved: false,
                },
                V4l2DeviceInfo {
                    path: "/dev/video12".into(),
                    name: "Dremes Capture".into(),
                    is_loopback: true,
                    is_capture: true,
                    is_output: true,
                    is_reserved: false,
                },
            ],
            ..Default::default()
        };
        let first = catalog.first_output_device().unwrap();
        assert_eq!(first.path, "/dev/video11");
        assert!(catalog.output_devices.iter().any(|d| !d.is_reserved));
    }

    #[test]
    fn run_state_defaults_idle() {
        assert_eq!(CaptureRunState::default(), CaptureRunState::Idle);
        assert!(!CaptureRunState::Idle.is_active());
        assert!(CaptureRunState::Starting.is_active());
        assert!(CaptureRunState::Active.is_active());
        assert!(CaptureRunState::Stopping.is_active());
        assert!(!CaptureRunState::Error.is_active());
    }
}