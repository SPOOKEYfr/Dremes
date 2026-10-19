//! The Linux backend implementing the PlatformBackend trait.

use dremes_core::capture::{CaptureCatalog, CaptureRequest, SessionInfo};
use dremes_core::error::Result;
use dremes_core::platform::{
    CaptureConfig, DiagnosticItem, GeneralCaptureStart, PlatformBackend, ResourceSnapshot, WifiInfo,
    WaydroidStatus,
};
use std::sync::Mutex;

use crate::{
    adb, apps, capture, capture_src, diagnostics, obs, resources::CpuHistory, v4l2, waydroid, wifi,
};

/// Linux implementation of `PlatformBackend`.
#[derive(Debug)]
pub struct LinuxBackend {
    /// The V4L2 device Dremes owns (auto-detected, configurable).
    device: String,
    /// Per-process CPU sampling state (interior mutability: `resource_snapshot`
    /// takes `&self`).
    cpu_history: Mutex<CpuHistory>,
}

impl Default for LinuxBackend {
    fn default() -> Self {
        Self {
            device: v4l2::DEFAULT_DEVICE.to_string(),
            cpu_history: Mutex::new(CpuHistory::new()),
        }
    }
}

impl LinuxBackend {
    /// Create a backend with the default capture device.
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            cpu_history: Mutex::new(CpuHistory::new()),
        }
    }
}

impl PlatformBackend for LinuxBackend {
    fn name(&self) -> &'static str {
        "Linux"
    }

    fn capture_device(&self) -> String {
        self.device.clone()
    }

    fn capture_device_exists(&self, device: &str) -> bool {
        v4l2::device_exists(device)
    }

    fn run_diagnostics(&self) -> Vec<DiagnosticItem> {
        diagnostics::run()
    }

    fn waydroid_status(&self) -> Result<WaydroidStatus> {
        Ok(waydroid::status())
    }

    fn start_waydroid_session(&self) -> Result<()> {
        waydroid::start()
    }

    fn stop_waydroid_session(&self) -> Result<()> {
        waydroid::stop_session()
    }

    fn discover_adb_endpoint(&self) -> Result<Option<String>> {
        adb::discover_endpoint(10)
    }

    fn ensure_adb_connected(&self, endpoint: &str) -> Result<()> {
        adb::ensure_connected(endpoint, 10)
    }

    fn obs_installed(&self) -> bool {
        obs::installed()
    }

    fn obs_running(&self) -> bool {
        obs::running()
    }

    fn open_obs(&self) -> Result<bool> {
        obs::open()
    }

    fn close_obs(&self) -> Result<bool> {
        obs::close()
    }

    fn detect_capture_config(&self) -> Result<CaptureConfig> {
        v4l2::read_config(&self.device)
    }

    fn validate_v4l2(&self, device: &str) -> Result<CaptureConfig> {
        v4l2::read_config(device)
    }

    fn verify_frames(&self, device: &str, seconds: u32) -> Result<u64> {
        v4l2::verify_frames(device, seconds)
    }

    fn start_capture(
        &self,
        device: &str,
    ) -> Result<Box<dyn dremes_core::platform::CaptureHandle>> {
        capture::start_handle(device)
    }

    fn capture_catalog(&self) -> CaptureCatalog {
        let session = capture_src::session_info();
        let all = capture_src::list_v4l2_devices(&self.device);
        let output_devices = all
            .iter()
            .filter(|d| !d.is_reserved && d.is_output)
            .cloned()
            .collect();
        let source_devices = all.clone();
        let mobile_devices = capture_src::mobile_devices();
        let backend_status = capture_src::backend_status(&session);
        CaptureCatalog {
            session,
            backend_status,
            output_devices,
            source_devices,
            mobile_devices,
        }
    }

    fn session_info(&self) -> SessionInfo {
        capture_src::session_info()
    }

    fn start_general_capture(&self, request: &CaptureRequest) -> Result<GeneralCaptureStart> {
        capture_src::start(request, &self.device)
    }

    fn stop_general_capture(&self) -> Result<()> {
        // The Engine owns the single general-capture handle; nothing further
        // to release at the backend layer.
        Ok(())
    }

    fn capture_diagnosis(&self) -> dremes_core::capture::CaptureDiagnosis {
        capture_src::diagnosis(&self.device)
    }

    fn portal_source_types(&self) -> Option<u32> {
        capture_src::portal_source_types()
    }

    fn resource_snapshot(&self) -> ResourceSnapshot {
        let mut history = self.cpu_history.lock().unwrap();
        crate::resources::snapshot(&self.device, &mut history)
    }

    fn resource_telemetry(&self) -> ResourceSnapshot {
        crate::resources::telemetry()
    }

    fn wifi_info(&self) -> Option<WifiInfo> {
        wifi::info()
    }

    fn wifi_boost(&self, on: bool) -> Result<String> {
        wifi::boost(on)
    }

    fn super_close(&self) -> Result<String> {
        apps::super_close()
    }
}
