//! Dremes Windows backend.
//!
//! Waydroid is a Linux-only Android environment. On Windows we do NOT pretend
//! it is available or implement capture. The platform backend reports a clear,
//! graceful message. The shared Dremes application (GUI) still runs.

use dremes_core::error::{DremesError, Result};
use dremes_core::platform::{
    CaptureConfig, DiagnosticItem, PlatformBackend, ResourceSnapshot, WaydroidStatus,
};

/// Windows implementation of `PlatformBackend`.
///
/// All Waydroid/capture operations return a clearly-worded "unavailable on
/// Windows" error. On Windows, `std::env::consts::OS` is "windows".
pub struct WindowsBackend;

impl Default for WindowsBackend {
    fn default() -> Self {
        Self
    }
}

impl PlatformBackend for WindowsBackend {
    fn name(&self) -> &'static str {
        "Windows"
    }

    fn capture_device(&self) -> String {
        String::new()
    }

    fn capture_device_exists(&self, _device: &str) -> bool {
        false
    }

    fn run_diagnostics(&self) -> Vec<DiagnosticItem> {
        vec![DiagnosticItem {
            key: "waydroid".into(),
            label: "Waydroid".into(),
            status: "fail",
            message: "Waydroid management is unavailable on Windows.".into(),
        }]
    }

    fn waydroid_status(&self) -> Result<WaydroidStatus> {
        Err(DremesError::Message(
            "Waydroid management is unavailable on Windows.".into(),
        ))
    }

    fn start_waydroid_session(&self) -> Result<()> {
        Err(DremesError::Message(
            "Waydroid management is unavailable on Windows.".into(),
        ))
    }

    fn stop_waydroid_session(&self) -> Result<()> {
        Err(DremesError::Message(
            "Waydroid management is unavailable on Windows.".into(),
        ))
    }

    fn discover_adb_endpoint(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn ensure_adb_connected(&self, _endpoint: &str) -> Result<()> {
        Err(DremesError::Message(
            "Waydroid management is unavailable on Windows.".into(),
        ))
    }

    fn obs_installed(&self) -> bool {
        false
    }

    fn obs_running(&self) -> bool {
        false
    }

    fn open_obs(&self) -> Result<bool> {
        Ok(false)
    }

    fn detect_capture_config(&self) -> Result<CaptureConfig> {
        Err(DremesError::Message(
            "Waydroid capture is unavailable on Windows.".into(),
        ))
    }

    fn validate_v4l2(&self, _device: &str) -> Result<CaptureConfig> {
        Err(DremesError::Message(
            "V4L2 capture is unavailable on Windows.".into(),
        ))
    }

    fn verify_frames(&self, _device: &str, _seconds: u32) -> Result<u64> {
        Err(DremesError::Message(
            "V4L2 capture is unavailable on Windows.".into(),
        ))
    }

    fn start_capture(
        &self,
        _device: &str,
    ) -> Result<Box<dyn dremes_core::platform::CaptureHandle>> {
        Err(DremesError::Message(
            "V4L2 capture is unavailable on Windows.".into(),
        ))
    }

    fn resource_snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot::default()
    }
}
