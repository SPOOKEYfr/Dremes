//! The Dremes Engine.
//!
//! The Engine is the single background coordinator. It owns the state machine,
//! the process registry, and the platform backend. All starts/stops of
//! Waydroid, adb, scrcpy flow through here — there is exactly one engine, so
//! there is never more than one capture writer, and nothing fights over
//! ADB/scrcpy/FFmpeg//dev/video10.
//!
//! The Engine keeps running even if the GUI closes; the GUI only sends
//! intents (Commands) and reads State snapshots.

use std::sync::{Arc, Mutex};

use dremes_core::capture::{CaptureCatalog, CaptureDiagnosis, CaptureRequest, CaptureRunState, CaptureSessionInfo};
use dremes_core::error::{DremesError, Result};
use dremes_core::platform::{PlatformBackend, ResourceSnapshot, WaydroidStatus};
use dremes_core::state::{
    CaptureState, EngineState, Mode, StreamingState, SystemState, WaydroidState,
};
use crate::ipc::{Ack, Command, LogEvent, Response, Snapshot};
use crate::registry::ProcessRegistry;

/// Internal, shared state of the engine.
struct EngineInner {
    status: SystemState,
    registry: ProcessRegistry,
    backend: Box<dyn PlatformBackend + Send + Sync>,
    resources: ResourceSnapshot,

    /// The single live capture writer. Invariant: at most one.
    capture: Option<Box<dyn dremes_core::platform::CaptureHandle>>,

    /// The capture device in use (kept alongside the handle for reporting).
    capture_device: String,

    /// The single active general-purpose capture writer (independent of the
    /// Waydroid pipeline). Invariant: at most one.
    general_capture: Option<Box<dyn dremes_core::capture::GeneralCaptureHandle>>,
    /// The registered pid of the active general capture (for registry cleanup).
    general_capture_pid: u32,

    /// Ring buffer of recent log events, shared with the GUI.
    log: std::collections::VecDeque<crate::ipc::LogEvent>,

    /// Automatic-recovery backoff / retry tracking (per-episode).
    recovery: RecoveryTracker,

    /// Stream Boost active flag + applied optimizations note.
    stream_boost: bool,
    stream_boost_note: String,

    /// Game Boost active flag + applied optimizations note.
    game_boost: bool,
    game_boost_note: String,
}

/// Maximum number of log events kept in the in-memory ring buffer.
const LOG_TAIL: usize = 200;

/// Bounds for the automatic-recovery worker so it can never thrash the system
/// by re-issuing intents in a tight loop. Recovery actions are rate-limited to
/// at most one attempt per cooldown window, and per-episode: after `limit`
/// consecutive failed attempts (without the component becoming healthy) the
/// worker gives up and waits for a manual action or a healthy state change.
#[derive(Default)]
struct RecoveryTracker {
    /// When the last recovery action was performed (for backoff).
    last_attempt: Option<std::time::Instant>,
    /// Consecutive failed recovery attempts in the current episode.
    episode_attempts: u32,
}

/// Minimum time between automatic recovery actions (seconds).
const RECOVERY_COOLDOWN_SECS: u64 = 5;

/// The Engine handle. Cheap to clone (shares the inner state).
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Mutex<EngineInner>>,
}

impl Engine {
    /// Build a new Engine with the given backend.
    pub fn new(backend: impl PlatformBackend + Send + Sync + 'static) -> Self {
        let inner = EngineInner {
            status: SystemState::default(),
            registry: ProcessRegistry::new(),
            backend: Box::new(backend),
            resources: ResourceSnapshot::default(),
            capture: None,
            capture_device: "/dev/video10".into(),
            general_capture: None,
            general_capture_pid: 0,
            log: std::collections::VecDeque::new(),
            recovery: RecoveryTracker::default(),
            stream_boost: false,
            stream_boost_note: String::new(),
            game_boost: false,
            game_boost_note: String::new(),
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    /// A snapshot of the current system state (for the GUI).
    pub fn snapshot(&self) -> SystemState {
        let guard = self.inner.lock().unwrap();
        guard.status.clone()
    }

    /// The current resource snapshot.
    pub fn resource_snapshot(&self) -> ResourceSnapshot {
        let guard = self.inner.lock().unwrap();
        guard.resources.clone()
    }

    /// Current automatic-recovery/reconnect count for this session.
    pub fn reconnect_count(&self) -> u32 {
        self.inner.lock().unwrap().status.reconnect_count
    }

    /// Whether an automatic recovery action is permitted right now: the
    /// cooldown since the last attempt has elapsed AND we have not exhausted
    /// this episode's retry budget.
    pub fn recovery_ready(&self, limit: u32) -> bool {
        let g = self.inner.lock().unwrap();
        if let Some(t) = g.recovery.last_attempt {
            if t.elapsed().as_secs() < RECOVERY_COOLDOWN_SECS {
                return false;
            }
        }
        g.recovery.episode_attempts < limit.max(1)
    }

    /// Whether this episode's recovery retry budget is exhausted.
    pub fn recovery_episode_exhausted(&self, limit: u32) -> bool {
        self.inner.lock().unwrap().recovery.episode_attempts >= limit.max(1)
    }

    /// Record that a recovery attempt was performed (updates the cooldown and
    /// the per-episode attempt counter) and return the new attempt count.
    pub fn mark_recovery_attempt(&self) -> u32 {
        let mut g = self.inner.lock().unwrap();
        g.recovery.last_attempt = Some(std::time::Instant::now());
        g.recovery.episode_attempts = g.recovery.episode_attempts.saturating_add(1);
        g.recovery.episode_attempts
    }

    /// Reset the per-episode recovery retry budget (called when the component
    /// is observed healthy again).
    pub fn reset_recovery_episode(&self) {
        let mut g = self.inner.lock().unwrap();
        g.recovery.episode_attempts = 0;
    }

    /// Whether a live capture writer handle exists and is alive.
    ///
    /// Returns `None` when no capture is active (or handle not yet assigned),
    /// `Some(true)` when a writer is alive, and `Some(false)` when a writer was
    /// expected but its process is gone. Used by the recovery worker.
    pub fn capture_handle_alive(&self) -> Option<bool> {
        let guard = self.inner.lock().unwrap();
        match &guard.capture {
            Some(h) => Some(h.is_alive()),
            None => {
                // If we are in streaming mode but have no handle, that's a
                // recovery signal; otherwise there is simply nothing to check.
                if guard.status.mode == Mode::Streaming {
                    Some(false)
                } else {
                    None
                }
            }
        }
    }

    /// Handle a command from the GUI. Returns an IPC response.
    pub fn handle_command(&self, cmd: Command) -> Result<Response> {
        match cmd {
            Command::EngineOn => { self.engine_on()?; self.ack() }
            Command::EngineOff => { self.engine_off()?; self.ack() }
            Command::GamingMode => { self.enter_gaming()?; self.ack() }
            Command::StartStreaming => { self.start_streaming()?; self.ack() }
            Command::StopStreaming => { self.stop_streaming()?; self.ack() }
            Command::StopWaydroid => { self.stop_waydroid()?; self.ack() }
            Command::ReconnectToObs => { self.reconnect_to_obs()?; self.ack() }
            Command::OpenObs => { self.open_obs()?; self.ack() }
            Command::CloseObs => { self.close_obs()?; self.ack() }
            Command::StreamBoost => {
                let on = !self.inner.lock().unwrap().status.stream_boost;
                self.set_stream_boost(on)?;
                self.ack()
            }
            Command::GameBoost => {
                let on = !self.inner.lock().unwrap().status.game_boost;
                self.set_game_boost(on)?;
                self.ack()
            }
            Command::WifiBoost => {
                let on = !self.inner.lock().unwrap().status.wifi_boost;
                self.set_wifi_boost(on)?;
                self.ack()
            }
            Command::Super => { self.super_action()?; self.ack() }
            Command::GetState => { self.refresh_resources().ok(); self.snap() }
            Command::GetResources => { self.refresh_resources()?; self.snap() }
            Command::Ping => self.ack(),
            Command::GetCaptureCatalog => self.catalog(),
            Command::StartCapture(request) => { self.start_general_capture(request)?; self.ack() }
            Command::StopCapture => { self.stop_general_capture()?; self.ack() }
            Command::GetCaptureStatus => self.capture_status(),
            Command::CaptureDiagnose => Ok(Response::CaptureDiagnosis(self.capture_diagnose())),
            Command::CaptureFix => Ok(Response::CaptureDiagnosis(self.capture_fix())),
        }
    }

    fn ack(&self) -> Result<Response> {
        Ok(Response::Ack(Ack { ok: true, error: None }))
    }

    fn snap(&self) -> Result<Response> {
        let guard = self.inner.lock().unwrap();
        Ok(Response::Snapshot(Snapshot {
            status: guard.status.clone(),
            resources: guard.resources.clone(),
            log_tail: guard.log.iter().map(|e| e.clone()).collect(),
        }))
    }

    /// Respond with the live capture catalog (session, devices, backends).
    fn catalog(&self) -> Result<Response> {
        self.refresh_resources().ok();
        let catalog: CaptureCatalog = {
            let g = self.inner.lock().unwrap();
            g.backend.capture_catalog()
        };
        Ok(Response::Catalog(catalog))
    }

    /// Respond with the current general-capture session status.
    fn capture_status(&self) -> Result<Response> {
        self.refresh_resources().ok();
        let status = { self.inner.lock().unwrap().status.capture_session.clone() };
        Ok(Response::CaptureStatus(status))
    }

    /// Health report of the capture stack (read-only, safe while engine OFF).
    fn capture_diagnose(&self) -> CaptureDiagnosis {
        self.reconcile_general_capture();
        let mut d = { let g = self.inner.lock().unwrap(); g.backend.capture_diagnosis() };
        {
            let g = self.inner.lock().unwrap();
            let st = &g.status.capture_session;
            if st.state == CaptureRunState::Error && !st.error.is_empty() {
                d.issues.push(format!(
                    "A previous capture ended in an error (\"{}\"). Fix All or Stop clears it.",
                    st.error
                ));
            }
        }
        d.healthy = d.issues.is_empty();
        d
    }

    /// Auto-repair what can be fixed without root, then re-diagnose.
    fn capture_fix(&self) -> CaptureDiagnosis {
        {
            let mut g = self.inner.lock().unwrap();
            if g.status.capture_session.state == CaptureRunState::Error {
                g.status.capture_session = CaptureSessionInfo::default();
                log::info!("CaptureFix: cleared stale capture error state");
            }
        }
        // Ensure the desktop portal is up (PipeWire screencast for OBS). `start`
        // is idempotent and never kills an active session.
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", "xdg-desktop-portal.service"])
            .status();
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", "xdg-desktop-portal-gnome.service"])
            .status();
        // A portal that is up but advertises 0 source types is stale: it synced
        // before its backend was ready, so OBS hides Screen Capture (PipeWire).
        // Restart it (user-scoped, no root) and let it re-sync from the backend.
        if let Some(0) = self.inner.lock().unwrap().backend.portal_source_types() {
            log::warn!("CaptureFix: portal reports 0 source types — restarting xdg-desktop-portal");
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "restart", "xdg-desktop-portal.service"])
                .status();
        }
        let d = self.capture_diagnose();
        if d.healthy {
            self.record_log("capture", "info", "Capture Fix All: no outstanding issues.");
        } else {
            self.record_log(
                "capture",
                "warn",
                &format!("Capture Fix All done. {} issue(s) remain.", d.issues.len()),
            );
        }
        d
    }

    /// Append a log event to the in-memory ring buffer.
    pub fn record_log(&self, component: &str, level: &str, message: &str) {
        let mut guard = self.inner.lock().unwrap();
        guard.log.push_back(LogEvent {
            timestamp: dremes_core::version::now_iso8601(),
            level: level.into(),
            component: component.into(),
            message: message.into(),
        });
        while guard.log.len() > LOG_TAIL {
            guard.log.pop_front();
        }
    }

    // ---- Engine master switch ----

    fn engine_on(&self) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.status.engine = EngineState::Starting;
        g.status.engine_enabled = true;
        log::info!("Engine starting");
        drop(g);
        self.record_log("engine", "info", "Engine starting");

        // Do a quick diagnostics/status probe to establish a baseline.
        let status = self.waydroid_status_inner();
        let mut g = self.inner.lock().unwrap();
        g.status.waydroid = map_waydroid_status(&status);
        g.status.engine = EngineState::Running;
        log::info!("Engine running");
        drop(g);
        self.record_log("engine", "info", "Engine running");
        Ok(())
    }

    fn engine_off(&self) -> Result<()> {
        // Proper shutdown: stop the general-purpose capture first (it never
        // touches the Waydroid device), then stop streaming/capture, stop
        // recovery, keep Waydroid per policy, then stop the engine.
        let _ = self.stop_general_capture();

        {
            let mut g = self.inner.lock().unwrap();
            g.status.engine = EngineState::Stopping;
            log::info!("Engine stopping");

            // Stop capture and streaming.
            if g.capture.is_some() {
                if let Some(c) = g.capture.take() {
                    let _ = c.stop();
                }
            }
            g.status.capture = CaptureState::Off;
            g.status.streaming = StreamingState::Off;
        }
        self.record_log("engine", "info", "Engine stopping");

        {
            let mut g = self.inner.lock().unwrap();
            g.status.engine = EngineState::Off;
            g.status.engine_enabled = false;
            log::info!("Engine off");
        }
        self.record_log("engine", "info", "Engine off");
        Ok(())
    }

    // ---- Modes ----

    fn enter_gaming(&self) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
        }
        // Ensure waydroid is running.
        let status = self.waydroid_status_inner();
        if !status.running && !status.session_running {
            self.waydroid_start_inner()?;
        }

        let mut g = self.inner.lock().unwrap();
        // Ensure capture is OFF in Gaming mode (the important invariant).
        if g.capture.is_some() {
            let c = g.capture.take().unwrap();
            let _ = c.stop();
        }
        g.status.capture = CaptureState::Off;
        g.status.streaming = StreamingState::Off;
        g.status.mode = Mode::Gaming;
        g.status.adb_connected = false;
        g.status.adb_endpoint.clear();
        log::info!("Entered Gaming Mode (capture OFF)");
        drop(g);
        self.record_log("mode", "info", "Gaming Mode entered (capture OFF)");
        Ok(())
    }

    fn start_streaming(&self) -> Result<()> {
        {
            let mut g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
            g.status.streaming = StreamingState::Starting;
            g.status.mode = Mode::Streaming;
        }
        let device = self.capture_device_inner();
        let (frames, cfg) = self.start_streaming_inner(&device)?;

        let mut g = self.inner.lock().unwrap();
        g.status.streaming = StreamingState::Active;
        g.status.capture = CaptureState::Running;
        g.status.capture_info.width = cfg.width;
        g.status.capture_info.height = cfg.height;
        g.status.capture_info.format = cfg.format.clone();
        g.status.capture_info.device = device.clone();
        g.status.capture_info.fps = frames as f32;
        g.status.v4l2_detail = format!(
            "capture {}@{}x{} format={}; {} frames verified",
            device, cfg.width, cfg.height, cfg.format, frames
        );
        log::info!("Streaming active: {}x{} format={} on {device}", cfg.width, cfg.height, cfg.format);
        drop(g);
        Ok(())
    }

    /// The full START STREAMING sequence:
    /// CHECK WAYDROID -> START IF NEEDED -> WAIT READY -> DISCOVER ADB ->
    /// VERIFY ADB ONLINE -> DISCOVER RESOLUTION -> CHECK /dev/video10 ->
    /// CLEAN DREMES-OWNED STALE CAPTURE -> START HEADLESS SCRCPY V4L2 SINK ->
    /// VERIFY REAL FRAMES -> VERIFY /dev/video10 -> OBS READY.
    ///
    /// `start_streaming` never declares success before actual frames flow.
    fn start_streaming_inner(&self, device: &str) -> Result<(u64, dremes_core::platform::CaptureConfig)> {
        // 1. CHECK WAYDROID; START IF NEEDED; WAIT UNTIL READY.
        let wd = self.waydroid_status_inner();
        {
            let mut g = self.inner.lock().unwrap();
            g.status.waydroid = map_waydroid_status(&wd);
        }
        if !wd.running {
            self.record_log("streaming", "info", "Waydroid not running; starting it...");
            let mut g = self.inner.lock().unwrap();
            g.status.waydroid = WaydroidState::Starting;
            drop(g);
            self.waydroid_start_inner()?;
        }

        // 2. Rediscover ADB dynamically + verify online.
        let endpoint = self.adb_discover_inner()?;
        if endpoint.is_none() {
            return Err(DremesError::Adb(
                "Waydroid is running but no ADB device was discovered. Restart Waydroid or check ADB.".into(),
            ));
        }

        // 3. DISCOVER RESOLUTION from the output (writer) side of /dev/video10.
        let base_cfg = {
            let g = self.inner.lock().unwrap();
            g.backend.validate_v4l2(device)?
        };

        // 4. CHECK /dev/video10 present + capabilities.
        let device_exists = { let g = self.inner.lock().unwrap(); g.backend.capture_device_exists(&device.to_string()) };
        if !device_exists {
            return Err(DremesError::V4l2(format!(
                "{device} does not exist. Load v4l2loopback and ensure it owns {device}."
            )));
        }

        // 5. CLEAN ONLY DREMES-OWNED STALE CAPTURE PROCESSES.
        self.stop_capture_only()?;

        // 6. START HEADLESS SCRCPY V4L2 SINK (the single writer).
        {
            let mut g = self.inner.lock().unwrap();
            g.status.capture = CaptureState::Starting;
            let handle = g.backend.start_capture(device)?;
            g.capture = Some(handle);
            g.capture_device = device.into();
            let pid = g.capture.as_ref().unwrap().pid().unwrap_or(0);
            g.registry.register(dremes_core::process::OwnedProcess::new(
                pid,
                dremes_core::process::ProcessRole::Capture,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            ));
            g.status.capture = CaptureState::Running;
        }

        // Give scrcpy a moment to connect and start writing frames.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut frames_verified = 0u64;
        let mut verified_cfg = base_cfg;
        while std::time::Instant::now() < deadline {
            // 7. VERIFY REAL VIDEO FRAMES + capture-side format becomes valid.
            match self.capture_verify_inner(device) {
                Ok(frames) => {
                    frames_verified = frames;
                    // 8. Read the live capture-side format for accurate metadata.
                    // scrcpy can briefly negotiate a transitional format right
                    // after connecting, so poll until a sane steady-state format
                    // is reported.
                    let steady_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while std::time::Instant::now() < steady_deadline {
                        if let Some(cfg) = self.backend_stream_config(device) {
                            if cfg.width > 0 && cfg.height > 0 && cfg.format != "not-initialized" {
                                verified_cfg = cfg;
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                    break;
                }
                Err(e) => {
                    // Keep waiting unless the writer died.
                    let alive = self.capture_alive_hint();
                    if !alive {
                        return Err(DremesError::Capture(format!(
                            "Capture writer exited before producing frames: {e}"
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        if frames_verified == 0 {
            return Err(DremesError::V4l2(format!(
                "No real video frames detected on {device} within 20s. The capture producer did not deliver a valid stream."
            )));
        }

        Ok((frames_verified, verified_cfg))
    }

    fn backend_stream_config(&self, device: &str) -> Option<dremes_core::platform::CaptureConfig> {
        let g = self.inner.lock().unwrap();
        g.backend.validate_v4l2(device).ok()
    }

    fn capture_alive_hint(&self) -> bool {
        let g = self.inner.lock().unwrap();
        match &g.capture {
            Some(h) => h.is_alive(),
            None => false,
        }
    }

    fn stop_streaming(&self) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.status.streaming = StreamingState::Stopping;
        if let Some(c) = g.capture.take() {
            let _ = c.stop();
        }
        // Stop and clean up only Dremes-owned capture processes.
        let _ = crate::process_manager::stop_owned(
            &mut g.registry,
            |r| matches!(r, dremes_core::process::ProcessRole::Capture),
        );
        g.status.capture = CaptureState::Off;
        g.status.streaming = StreamingState::Off;
        g.status.adb_connected = false;
        g.status.mode = Mode::Gaming; // return to lightweight gaming state
        log::info!("Streaming stopped; Waydroid and game left running");
        drop(g);
        self.record_log("streaming", "info", "Streaming stopped; Waydroid and game left running");
        Ok(())
    }

    fn stop_waydroid(&self) -> Result<()> {
        // 1. Stop streaming/capture first.
        self.stop_streaming()?;

        // 2. Stop Dremes-owned ADB capture connections - nothing extra.

        // 3. Stop Waydroid session safely.
        {
            let mut g = self.inner.lock().unwrap();
            g.status.waydroid = WaydroidState::Stopping;
            drop(g);
        }
        self.waydroid_stop_safe()?;

        let mut g = self.inner.lock().unwrap();
        g.status.waydroid = WaydroidState::Stopped;
        g.status.mode = Mode::Idle;
        log::info!("Waydroid stopped");
        drop(g);
        self.record_log("waydroid", "info", "Waydroid stopped");
        Ok(())
    }

    fn reconnect_to_obs(&self) -> Result<()> {
        log::info!("Manual reconnect requested");

        fn engine_gate(e: &Engine) -> Result<()> {
            let g = e.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
            Ok(())
        }
        engine_gate(self)?;

        // 1. Probe Waydroid live.
        {
            let mut g = self.inner.lock().unwrap();
            g.status.streaming = StreamingState::Starting;
            g.status.mode = Mode::Streaming;
            drop(g);
        }
        let wd = self.waydroid_status_inner();
        {
            let mut g = self.inner.lock().unwrap();
            g.status.waydroid = map_waydroid_status(&wd);
        }
        if !wd.running {
            return Err(DremesError::Waydroid {
                message: "Waydroid is stopped.".into(),
                suggestion: "Start Waydroid first (Gaming Mode or Streaming Mode), or reconnect will start it for you.".into(),
            });
        }

        // 2. Rediscover ADB dynamically.
        let _ = self.adb_discover_inner()?;

        // 3. Stop only Dremes-owned stale capture processes, then restart.
        self.stop_capture_only()?;
        let device = self.capture_device_inner();
        let (frames, cfg) = self.start_streaming_inner(&device)?;

        {
            let mut g = self.inner.lock().unwrap();
            g.status.reconnect_count = g.status.reconnect_count.saturating_add(1);
            g.status.obs_status = if g.backend.obs_installed() { "RUNNING".into() } else { "NOT INSTALLED".into() };
            g.status.streaming = StreamingState::Active;
            g.status.capture = CaptureState::Running;
            g.status.capture_info.width = cfg.width;
            g.status.capture_info.height = cfg.height;
            g.status.capture_info.format = cfg.format.clone();
            g.status.capture_info.device = device.clone();
            g.status.capture_info.fps = frames as f32;
            drop(g);
        }
        log::info!("Reconnect to OBS complete");
        self.record_log("obs", "info", "Reconnect to OBS complete");
        Ok(())
    }

    fn open_obs(&self) -> Result<()> {
        let installed = {
            let g = self.inner.lock().unwrap();
            g.backend.obs_installed()
        };
        if !installed {
            return Err(DremesError::msg("OBS Studio is not installed or could not be launched."));
        }
        let opened = {
            let g = self.inner.lock().unwrap();
            g.backend.open_obs()?
        };
        if !opened {
            return Err(DremesError::msg("OBS Studio is not installed or could not be launched."));
        }
        self.record_log("obs", "info", "OBS opened/activated");
        Ok(())
    }

    fn close_obs(&self) -> Result<()> {
        let was_running = {
            let g = self.inner.lock().unwrap();
            g.backend.obs_running()
        };
        if !was_running {
            return Err(DremesError::msg("OBS Studio is not running."));
        }
        {
            let g = self.inner.lock().unwrap();
            g.backend.close_obs()?;
        }
        self.record_log("obs", "info", "OBS close requested");
        Ok(())
    }

    fn set_stream_boost(&self, on: bool) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
        }
        let mut g = self.inner.lock().unwrap();
        g.stream_boost = on;
        g.status.stream_boost = on;
        if on {
            // Safe optimization only: raise the capture (scrcpy) niceness so
            // its realtime-ish frame pump is less likely to be starved, but
            // never above the game. We do NOT over-prioritize Dremes.
            if let Some(pid) = g.capture.as_ref().and_then(|c| c.pid()) {
                let _ = crate::process_manager::sys::terminate_nice(pid, -5);
            }
            g.status.stream_boost_note = "scrcpy niceness -> -5 (capture priority; game untouched)".into();
            g.stream_boost_note = g.status.stream_boost_note.clone();
        } else {
            g.status.stream_boost_note.clear();
            g.stream_boost_note.clear();
        }
        log::info!("Stream Boost {}", if on { "ON" } else { "OFF" });
        Ok(())
    }

    fn set_game_boost(&self, on: bool) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
        }
        // Game Boost: reduce Dremes background overhead and only gently prefer
        // Waydroid. We never use max priority and never starve anything.
        let mut g = self.inner.lock().unwrap();
        g.game_boost = on;
        g.status.game_boost = on;
        if on {
            // When boosting for the game while NOT streaming, suspend capture.
            if g.status.streaming != StreamingState::Active && g.capture.is_some() {
                if let Some(c) = g.capture.take() {
                    let _ = c.stop();
                }
                g.status.capture = CaptureState::Off;
            }
            g.status.game_boost_note =
                "reduced Dremes sampling; capture suspended when not streaming".into();
            g.game_boost_note = g.status.game_boost_note.clone();
        } else {
            g.status.game_boost_note.clear();
            g.game_boost_note.clear();
        }
        log::info!("Game Boost {}", if on { "ON" } else { "OFF" });
        Ok(())
    }

    fn set_wifi_boost(&self, on: bool) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
        }
        let info = {
            let g = self.inner.lock().unwrap();
            g.backend.wifi_info()
        };
        let Some(info) = info else {
            return Err(DremesError::msg(
                "No wireless interface detected.",
            ));
        };
        let note = {
            let g = self.inner.lock().unwrap();
            g.backend.wifi_boost(on)?
        };
        {
            let mut g = self.inner.lock().unwrap();
            g.status.wifi_boost = on;
            g.status.wifi_iface = info.iface.clone();
            g.status.wifi_boost_note = note.clone();
        }
        log::info!("WiFi Boost {} — {}", if on { "ON" } else { "OFF" }, note);
        self.record_log("wifi", "info", &note);
        Ok(())
    }

    /// SUPER mode: close background GUI apps (engine must be ON).
    fn super_action(&self) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
        }
        let note = {
            let g = self.inner.lock().unwrap();
            g.backend.super_close()?
        };
        {
            let mut g = self.inner.lock().unwrap();
            g.status.super_report = note.clone();
        }
        log::info!("SUPER: {note}");
        self.record_log("super", "info", &note);
        Ok(())
    }

    /// Stop only Dremes-owned capture processes (never OBS / unrelated).
    fn stop_capture_only(&self) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(c) = g.capture.take() {
            let _ = c.stop();
        }
        let _ = crate::process_manager::stop_owned(
            &mut g.registry,
            |r| matches!(r, dremes_core::process::ProcessRole::Capture),
        );
        g.status.capture = CaptureState::Off;
        Ok(())
    }

    // ---- General-purpose capture (independent of the Waydroid pipeline) ----

    /// Start a general-purpose capture (Mobile / Game / Window / Monitor /
    /// Camera / V4L2 device) into a *free* loopback device that never shadows
    /// the Waydroid capture node.
    fn start_general_capture(&self, request: CaptureRequest) -> Result<()> {
        {
            let g = self.inner.lock().unwrap();
            if !g.status.engine_enabled {
                return Err(DremesError::msg("Engine is OFF. Enable it first."));
            }
            if g.general_capture.is_some() || g.status.capture_session.state.is_active() {
                return Err(DremesError::Capture(
                    "A general capture is already running; stop it first.".to_string(),
                ));
            }
        }

        // Device safety: the reserved Waydroid node can never be an output.
        let reserved = { let g = self.inner.lock().unwrap(); g.backend.capture_device() };
        dremes_core::capture::ensure_not_reserved(&request, &reserved)?;

        let now = unix_now();
        {
            let mut g = self.inner.lock().unwrap();
            g.status.capture_session = CaptureSessionInfo {
                state: CaptureRunState::Starting,
                kind: Some(request.kind),
                source: request.source.clone(),
                output_device: request.output_device.clone(),
                method: String::new(),
                connection: String::new(),
                width: request.width,
                height: request.height,
                fps: request.fps,
                started_at_unix: now,
                error: String::new(),
            };
            g.general_capture = None;
        }

        // Resolve + spawn in the backend without holding the engine lock.
        let started = {
            let g = self.inner.lock().unwrap();
            g.backend.start_general_capture(&request)
        };

        let start = match started {
            Ok(s) => s,
            Err(e) => {
                {
                    let mut g = self.inner.lock().unwrap();
                    g.status.capture_session.state = CaptureRunState::Idle;
                    g.status.capture_session.kind = None;
                    g.status.capture_session.source.clear();
                    g.status.capture_session.output_device.clear();
                    g.status.capture_session.method.clear();
                    g.status.capture_session.connection.clear();
                    g.status.capture_session.started_at_unix = 0;
                    g.status.capture_session.error = e.to_string();
                }
                return Err(e);
            }
        };

        // Defense-in-depth: the backend must never resolve to the reserved node.
        if dremes_core::capture::is_reserved_device(&start.output_device, &reserved) {
            let _ = start.handle.stop();
            let msg = "Backend resolved the reserved Waydroid device as a general capture output; refusing.";
            log::error!("{msg}");
            return Err(DremesError::Capture(msg.into()));
        }

        {
            let mut g = self.inner.lock().unwrap();
            if let Some(pid) = start.handle.pid() {
                g.general_capture_pid = pid;
                g.registry.register(dremes_core::process::OwnedProcess::new(
                    pid,
                    dremes_core::process::ProcessRole::GeneralCapture,
                    unix_now(),
                ));
            }
            g.general_capture = Some(start.handle);
            g.status.capture_session.state = CaptureRunState::Active;
            g.status.capture_session.output_device = start.output_device.clone();
            g.status.capture_session.method = start.method.clone();
            g.status.capture_session.connection = start.connection.clone();
            g.status.capture_session.source = start.source.clone();
            g.status.capture_session.width = start.width;
            g.status.capture_session.height = start.height;
            g.status.capture_session.fps = start.fps;
            g.status.capture_session.started_at_unix = now;
            g.status.capture_session.error.clear();
        }
        log::info!(
            "General capture started: {} → {}",
            start.method,
            start.output_device
        );
        self.record_log(
            "capture",
            "info",
            &format!("General capture started: {} → {}", start.method, start.output_device),
        );
        Ok(())
    }

    /// Stop the active general-purpose capture (only processes Dremes started).
    fn stop_general_capture(&self) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.status.capture_session.state = CaptureRunState::Stopping;
        let pid = g.general_capture_pid;
        if let Some(h) = g.general_capture.take() {
            // The handle verifies its own pid's command line before signaling.
            let _ = h.stop();
        }
        if pid != 0 {
            g.registry.unregister(pid);
        }
        g.general_capture_pid = 0;
        g.status.capture_session = CaptureSessionInfo::default();
        Ok(())
    }

    /// Reconcile the general-capture session with reality: a writer that died
    /// unexpectedly is reported as an honest Error instead of a stale Active.
    fn reconcile_general_capture(&self) {
        let mut g = self.inner.lock().unwrap();
        let active = matches!(g.status.capture_session.state, CaptureRunState::Active);
        let have_handle = g.general_capture.is_some();
        if have_handle {
            let dead = !g.general_capture.as_ref().unwrap().is_alive();
            if dead {
                let pid = g.general_capture_pid;
                if pid != 0 {
                    g.registry.unregister(pid);
                }
                g.general_capture_pid = 0;
                g.general_capture = None;
                g.status.capture_session.state = CaptureRunState::Error;
                g.status.capture_session.error = "Capture process exited unexpectedly.".into();
            }
        } else if active {
            g.status.capture_session.state = CaptureRunState::Error;
            g.status.capture_session.error = "Capture process exited unexpectedly.".into();
        }
    }

    // ---- Platform-driven helpers (thin wrappers that don't hold the lock) ----

    fn waydroid_status_inner(&self) -> WaydroidStatus {
        let g = self.inner.lock().unwrap();
        g.backend.waydroid_status().unwrap_or_else(|_| WaydroidStatus {
            running: false,
            session_running: false,
            container_running: false,
            vendor_type: String::new(),
            detail: "unknown".into(),
            container_state: String::new(),
            session_state: String::new(),
        })
    }

    fn waydroid_start_inner(&self) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.status.waydroid = WaydroidState::Starting;
        drop(g);
        log::info!("Starting Waydroid...");
        let r = {
            let g = self.inner.lock().unwrap();
            g.backend.start_waydroid_session()
        };
        let mut g = self.inner.lock().unwrap();
        match r {
            Ok(_) => {
                g.status.waydroid = WaydroidState::Running;
                Ok(())
            }
            Err(e) => {
                g.status.waydroid = WaydroidState::Error;
                Err(e)
            }
        }
    }

    fn waydroid_stop_safe(&self) -> Result<()> {
        let g = self.inner.lock().unwrap();
        g.backend.stop_waydroid_session()
    }

    fn adb_discover_inner(&self) -> Result<Option<String>> {
        log::info!("Discovering ADB endpoint...");
        let mut g = self.inner.lock().unwrap();
        let endpoint = g.backend.discover_adb_endpoint()?;
        if let Some(ep) = endpoint.as_deref() {
            match g.backend.ensure_adb_connected(ep) {
                Ok(()) => g.status.adb_connected = true,
                Err(_) => g.status.adb_connected = false,
            }
            g.status.adb_endpoint = ep.to_string();
        } else {
            g.status.adb_connected = false;
            g.status.adb_endpoint.clear();
        }
        Ok(endpoint)
    }

    fn capture_device_inner(&self) -> String {
        let g = self.inner.lock().unwrap();
        g.backend.capture_device()
    }

    fn capture_verify_inner(&self, device: &str) -> Result<u64> {
        let g = self.inner.lock().unwrap();
        g.backend.verify_frames(device, 2)
    }

    fn refresh_resources(&self) -> Result<()> {
        // Always reconcile the real Waydroid state so the GUI can never show a
        // stale "RUNNING" when Waydroid was stopped outside Dremes.
        let wd = self.waydroid_status_inner();
        {
            let mut g = self.inner.lock().unwrap();
            g.status.waydroid = map_waydroid_status(&wd);
            g.status.waydroid_session = wd.session_state.clone();
            g.status.waydroid_container = wd.container_state.clone();
            // Keep the reported wireless interface in sync with reality.
            match g.backend.wifi_info() {
                Some(info) => g.status.wifi_iface = info.iface,
                None => g.status.wifi_iface.clear(),
            }
            if g.status.waydroid == WaydroidState::Running && !g.status.adb_connected {
                // Re-attempt ADB discovery only when Waydroid just came up.
            }
            if g.status.waydroid != WaydroidState::Running {
                g.status.adb_connected = false;
            }
            // Reflect real OBS status.
            let installed = g.backend.obs_installed();
            let running = g.backend.obs_running();
            g.status.obs_status = if !installed {
                "NOT INSTALLED".into()
            } else if running {
                "RUNNING".into()
            } else {
                "NOT RUNNING".into()
            };
            // Keep the detected session environment fresh.
            g.status.session = g.backend.session_info();
            // Keep capture metadata truthful at steady state: re-read the live
            // format while streaming so a transient init format (0x0/BGR4) is
            // never shown, and keep the verified-frame count from startup.
            if g.status.streaming == StreamingState::Active
                && g.status.capture == CaptureState::Running
                && !g.status.capture_info.device.is_empty()
            {
                if let Ok(cfg) = g.backend.validate_v4l2(&g.status.capture_info.device) {
                    if cfg.width > 0 && cfg.height > 0 && !cfg.format.is_empty() && cfg.format != "not-initialized"
                    {
                        let dev = cfg.device.clone();
                        let frames = g.status.capture_info.fps as u64;
                        g.status.capture_info.width = cfg.width;
                        g.status.capture_info.height = cfg.height;
                        g.status.capture_info.format = cfg.format.clone();
                        g.status.capture_info.device = cfg.device.clone();
                        g.status.v4l2_detail = format!(
                            "capture {}@{}x{} format={}; {} frames verified",
                            dev, cfg.width, cfg.height, cfg.format, frames
                        );
                    }
                }
            }
        }
        // Reconcile the general-purpose capture session with reality (a writer
        // that exited is reported as an honest Error, never a stale Active).
        self.reconcile_general_capture();
        // CPU/RAM sampling only while the engine is on (per design). The
        // read-only hardware telemetry is always kept fresh so the Resources
        // page shows temperatures, GPU and fan info even when the engine is OFF.
        if !self.inner.lock().unwrap().status.engine_enabled {
            let t = self.inner.lock().unwrap().backend.resource_telemetry();
            let mut g = self.inner.lock().unwrap();
            g.resources.cpu_temp_c = t.cpu_temp_c;
            g.resources.ssd_temp_c = t.ssd_temp_c;
            g.resources.fan_rpm = t.fan_rpm;
            g.resources.battery_pct = t.battery_pct;
            g.resources.gpu_util_pct = t.gpu_util_pct;
            g.resources.gpu_temp_c = t.gpu_temp_c;
            return Ok(());
        }
        let snap = self.inner.lock().unwrap().backend.resource_snapshot();
        self.inner.lock().unwrap().resources = snap;
        Ok(())
    }
}

/// Map a live Waydroid probe into a definitive WaydroidState.
///
/// We never guess from a stale value — this purely derives from the probe's
/// session AND container state strings. It distinguishes explicit STARTING /
/// STOPPING transitions, hard errors, and Unknown when we cannot tell.
fn map_waydroid_status(s: &WaydroidStatus) -> WaydroidState {
    // A fully confirmed running state (session AND container both live).
    if s.running {
        return WaydroidState::Running;
    }

    let session = s.session_state.to_lowercase();
    let container = s.container_state.to_lowercase();

    // Explicit transitions.
    if session.contains("starting") || session.contains("stopping")
        || container.contains("activating") || container.contains("deactivating")
    {
        return if session.contains("stopping") || container.contains("deactivating") {
            WaydroidState::Stopping
        } else {
            WaydroidState::Starting
        };
    }

    // The session probe is authoritative: if `waydroid status` ran and the
    // session is definitively stopped, then Waydroid is not running. The systemd
    // `waydroid-container` unit frequently remains "active" (idle) after a stop,
    // so it must NOT be used to infer a running/starting Waydroid.
    if session == "stopped" {
        return WaydroidState::Stopped;
    }

    // Session not yet stopped but container is up: still coming up.
    if s.container_running && !s.session_running {
        return WaydroidState::Starting;
    }

    // Any other definitive stopped signal.
    if session.contains("stopped") && !container.contains("running") {
        return WaydroidState::Stopped;
    }

    // If the probe reported neither active nor a clean stopped, and we could
    // run waydroid status at all, it's an error or unknown.
    if !session.is_empty() || !container.is_empty() {
        WaydroidState::Unknown
    } else {
        WaydroidState::Stopped
    }
}

impl EngineInner {
}

/// Current Unix-epoch seconds (used for capture session timestamps).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for Engine {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        {
            let backend = dremes_linux_backend::LinuxBackend::new(
                dremes_linux_backend::v4l2::DEFAULT_DEVICE,
            );
            Engine::new(backend)
        }
        #[cfg(target_os = "windows")]
        {
            let backend = dremes_windows_backend::WindowsBackend::default();
            Engine::new(backend)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unimplemented!("Unsupported platform")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dremes_core::capture::{
        CaptureCatalog, CaptureRequest, CaptureRunState, CaptureSourceKind, GeneralCaptureHandle,
        SessionInfo,
    };
    use dremes_core::platform::{
        CaptureConfig, DiagnosticItem, GeneralCaptureStart, ResourceSnapshot, WaydroidStatus,
    };

    /// A mock backend that simulates the Linux behavior without touching real
    /// hardware, so core state transitions can be tested safely.
    #[derive(Default)]
    struct MockBackend {
        pub waydroid_on: std::sync::Mutex<bool>,
        pub capture_ok: bool,
        pub general_ok: std::sync::Mutex<bool>,
    }

    /// A no-op capture handle for tests.
    struct MockCapture {}

    impl dremes_core::platform::CaptureHandle for MockCapture {
        fn pid(&self) -> Option<u32> {
            Some(9999)
        }
        fn is_alive(&self) -> bool {
            true
        }
        fn stop(&self) -> Result<()> {
            Ok(())
        }
    }

    /// A no-op general-capture handle for tests.
    struct MockGeneralCapture {}

    impl GeneralCaptureHandle for MockGeneralCapture {
        fn pid(&self) -> Option<u32> {
            Some(7777)
        }
        fn is_alive(&self) -> bool {
            true
        }
        fn stop(&self) -> Result<()> {
            Ok(())
        }
    }

    impl PlatformBackend for MockBackend {
        fn name(&self) -> &'static str {
            "mock"
        }
        fn capture_device(&self) -> String {
            "/dev/video10".into()
        }
        fn capture_device_exists(&self, _d: &str) -> bool {
            true
        }
        fn run_diagnostics(&self) -> Vec<DiagnosticItem> {
            vec![]
        }
        fn waydroid_status(&self) -> Result<WaydroidStatus> {
            let on = *self.waydroid_on.lock().unwrap();
            Ok(WaydroidStatus {
                running: on,
                session_running: on,
                container_running: on,
                vendor_type: "MAINLINE".into(),
                detail: if on { "Session:	RUNNING".into() } else { "Session:	STOPPED".into() },
                container_state: if on { "running".into() } else { "stopped".into() },
                session_state: if on { "running".into() } else { "stopped".into() },
            })
        }
        fn start_waydroid_session(&self) -> Result<()> {
            *self.waydroid_on.lock().unwrap() = true;
            Ok(())
        }
        fn stop_waydroid_session(&self) -> Result<()> {
            *self.waydroid_on.lock().unwrap() = false;
            Ok(())
        }
        fn discover_adb_endpoint(&self) -> Result<Option<String>> {
            if *self.waydroid_on.lock().unwrap() {
                Ok(Some("10.0.0.2:5555".into()))
            } else {
                Ok(None)
            }
        }
        fn ensure_adb_connected(&self, _ep: &str) -> Result<()> {
            Ok(())
        }
        fn obs_installed(&self) -> bool {
            true
        }
        fn obs_running(&self) -> bool {
            true
        }
        fn open_obs(&self) -> Result<bool> {
            Ok(true)
        }
        fn detect_capture_config(&self) -> Result<CaptureConfig> {
            Ok(CaptureConfig {
                width: 1366,
                height: 738,
                format: "YU12".into(),
                device: "/dev/video10".into(),
                fps: 30,
            })
        }
        fn validate_v4l2(&self, _dev: &str) -> Result<CaptureConfig> {
            self.detect_capture_config()
        }
        fn verify_frames(&self, _dev: &str, _sec: u32) -> Result<u64> {
            if self.capture_ok {
                Ok(60)
            } else {
                Err(DremesError::msg("no frames"))
            }
        }
        fn start_capture(
            &self,
            _device: &str,
        ) -> Result<Box<dyn dremes_core::platform::CaptureHandle>> {
            Ok(Box::new(MockCapture {}))
        }
        fn capture_catalog(&self) -> CaptureCatalog {
            CaptureCatalog {
                session: SessionInfo::detect(Some("x11"), Some("XFCE"), None, Some(":0")),
                ..Default::default()
            }
        }
        fn session_info(&self) -> SessionInfo {
            self.capture_catalog().session
        }
        fn start_general_capture(&self, request: &CaptureRequest) -> Result<GeneralCaptureStart> {
            if !*self.general_ok.lock().unwrap() {
                return Err(DremesError::msg("general capture backend unavailable (test)"));
            }
            Ok(GeneralCaptureStart {
                handle: Box::new(MockGeneralCapture {}),
                output_device: "/dev/video11".into(),
                method: "mock".into(),
                connection: String::new(),
                source: request.source.clone(),
                width: request.width,
                height: request.height,
                fps: request.fps,
            })
        }
        fn resource_snapshot(&self) -> ResourceSnapshot {
            ResourceSnapshot::default()
        }
    }

    fn mock() -> Engine {
        Engine::new(MockBackend {
            waydroid_on: std::sync::Mutex::new(false),
            capture_ok: true,
            general_ok: std::sync::Mutex::new(true),
        })
    }

    #[test]
    fn engine_off_refuses_streaming() {
        let e = mock();
        assert!(e.handle_command(Command::StartStreaming).is_err());
        assert_eq!(e.snapshot().engine, EngineState::Off);
    }

    #[test]
    fn gaming_mode_starts_waydroid_no_capture() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        e.handle_command(Command::GamingMode).unwrap();
        let s = e.snapshot();
        assert_eq!(s.mode, Mode::Gaming);
        assert_eq!(s.waydroid, WaydroidState::Running);
        assert_eq!(s.capture, CaptureState::Off);
        assert_eq!(s.streaming, StreamingState::Off);
    }

    #[test]
    fn streaming_mode_runs_capture() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        e.handle_command(Command::StartStreaming).unwrap();
        let s = e.snapshot();
        assert_eq!(s.streaming, StreamingState::Active);
        assert_eq!(s.capture, CaptureState::Running);
        assert!(s.adb_endpoint.contains(":5555"));
    }

    #[test]
    fn stop_streaming_keeps_waydroid_and_stops_capture() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        e.handle_command(Command::StartStreaming).unwrap();
        e.handle_command(Command::StopStreaming).unwrap();
        let s = e.snapshot();
        assert_eq!(s.capture, CaptureState::Off);
        assert_eq!(s.streaming, StreamingState::Off);
        assert_eq!(s.waydroid, WaydroidState::Running);
        assert_eq!(s.mode, Mode::Gaming);
    }

    #[test]
    fn stop_waydroid_stops_everything() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        e.handle_command(Command::StartStreaming).unwrap();
        e.handle_command(Command::StopWaydroid).unwrap();
        let s = e.snapshot();
        assert_eq!(s.waydroid, WaydroidState::Stopped);
        assert_eq!(s.capture, CaptureState::Off);
        assert_eq!(s.streaming, StreamingState::Off);
        assert_eq!(s.mode, Mode::Idle);
    }

    #[test]
    fn reconnect_requires_engine_and_waydroid() {
        let e = mock();
        // Engine off -> reconnect fails.
        assert!(e.handle_command(Command::ReconnectToObs).is_err());
        // Engine on + gaming (waydroid running) -> reconnect works.
        e.handle_command(Command::EngineOn).unwrap();
        e.handle_command(Command::GamingMode).unwrap();
        e.handle_command(Command::ReconnectToObs).unwrap();
        let s = e.snapshot();
        assert_eq!(s.streaming, StreamingState::Active);
    }

    // ---- Recovery worker tests ----

    #[test]
    fn recovery_is_noop_when_engine_off() {
        let e = mock();
        e.recovery_pass();
        let s = e.snapshot();
        assert_eq!(s.mode, Mode::Idle);
        assert_eq!(s.reconnect_count, 0);
    }

    #[test]
    fn recovery_restores_waydroid_in_gaming_mode() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        // In Idle mode recovery does nothing; drive into Gaming first.
        e.handle_command(Command::GamingMode).unwrap();
        // With Waydroid running and stable, recovery leaves it untouched.
        e.recovery_pass();
        assert_eq!(e.snapshot().waydroid, WaydroidState::Running);
        assert_eq!(e.snapshot().mode, Mode::Gaming);
    }

    #[test]
    fn recovery_backs_off_when_component_never_becomes_healthy() {
        // Recovery must never re-issue intents in a tight loop: after the first
        // attempt the cooldown blocks immediate retries, and the per-episode
        // budget gives up once a limit of consecutive failures is reached.
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();

        // Cooldown clear and budget 3 idle -> ready for a first attempt.
        assert!(e.recovery_ready(3));
        e.mark_recovery_attempt();
        // One attempt has now been recorded.
        assert!(e.recovery_episode_exhausted(1));
        assert!(!e.recovery_episode_exhausted(2));

        // Immediately attempting again is blocked by the cooldown, and calling
        // many rapid passes does NOT add more attempts.
        assert!(!e.recovery_ready(3));
        for _ in 0..5 {
            e.recovery_pass();
        }
        assert!(e.recovery_episode_exhausted(1), "still exactly one attempt");
        assert!(!e.recovery_episode_exhausted(2), "no second attempt despite storm");

        // Once the component is observed healthy the budget resets.
        e.reset_recovery_episode();
        assert!(!e.recovery_episode_exhausted(1));
    }

    // ---- General-purpose capture tests ----

    #[test]
    fn general_capture_requires_engine_on() {
        let e = mock();
        let req = CaptureRequest::empty(CaptureSourceKind::Monitor);
        assert!(e.handle_command(Command::StartCapture(req)).is_err());
        assert_eq!(e.snapshot().capture_session.state, CaptureRunState::Idle);
    }

    #[test]
    fn general_capture_starts_and_stops() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        let req = CaptureRequest {
            kind: CaptureSourceKind::Camera,
            source: "/dev/video0".into(),
            output_device: "/dev/video11".into(),
            width: 1280,
            height: 720,
            fps: 30,
        };
        e.handle_command(Command::StartCapture(req)).unwrap();
        let s = e.snapshot();
        assert_eq!(s.capture_session.state, CaptureRunState::Active);
        assert_eq!(s.capture_session.output_device, "/dev/video11");
        assert_eq!(s.capture_session.method, "mock");
        assert_eq!(s.capture_session.width, 1280);

        // Stop it.
        e.handle_command(Command::StopCapture).unwrap();
        let s = e.snapshot();
        assert_eq!(s.capture_session.state, CaptureRunState::Idle);
        assert!(s.capture_session.output_device.is_empty());
    }

    #[test]
    fn general_capture_device_safety_blocks_waydroid_node() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        let req = CaptureRequest {
            kind: CaptureSourceKind::Monitor,
            source: String::new(),
            output_device: "/dev/video10".into(),
            width: 0,
            height: 0,
            fps: 0,
        };
        let r = e.handle_command(Command::StartCapture(req));
        assert!(r.is_err());
        assert_eq!(e.snapshot().capture_session.state, CaptureRunState::Idle);
    }

    #[test]
    fn engine_off_stops_general_capture() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        let req = CaptureRequest::empty(CaptureSourceKind::Camera);
        e.handle_command(Command::StartCapture(req)).unwrap();
        assert_eq!(e.snapshot().capture_session.state, CaptureRunState::Active);
        e.handle_command(Command::EngineOff).unwrap();
        assert_eq!(e.snapshot().capture_session.state, CaptureRunState::Idle);
    }

    #[test]
    fn double_start_is_rejected() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        let req = CaptureRequest::empty(CaptureSourceKind::Camera);
        e.handle_command(Command::StartCapture(req)).unwrap();
        assert!(e.handle_command(Command::StartCapture(CaptureRequest::empty(CaptureSourceKind::Monitor))).is_err());
        e.handle_command(Command::StopCapture).unwrap();
    }

    #[test]
    fn catalog_query_and_status_query() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        let cat = e.handle_command(Command::GetCaptureCatalog).unwrap();
        match cat {
            Response::Catalog(c) => {
                assert_eq!(c.session.desktop, "xfce");
                assert!(c.output_devices.is_empty());
            }
            other => panic!("expected Catalog, got {other:?}"),
        }
        let st = e.handle_command(Command::GetCaptureStatus).unwrap();
        match st {
            Response::CaptureStatus(s) => assert_eq!(s.state, CaptureRunState::Idle),
            other => panic!("expected CaptureStatus, got {other:?}"),
        }
    }

    #[test]
    fn diagnose_is_healthy_by_default() {
        let e = mock();
        let resp = e.handle_command(Command::CaptureDiagnose).unwrap();
        match resp {
            Response::CaptureDiagnosis(d) => {
                assert!(d.healthy);
                assert!(d.issues.is_empty());
            }
            other => panic!("expected CaptureDiagnosis, got {other:?}"),
        }
    }

    #[test]
    fn fix_clears_stale_error_state() {
        let e = mock();
        e.handle_command(Command::EngineOn).unwrap();
        // Simulate a writer that died -> Error state persists in the engine.
        {
            let mut g = e.inner.lock().unwrap();
            g.status.capture_session.state = CaptureRunState::Error;
            g.status.capture_session.error = "Capture process exited unexpectedly.".into();
        }
        let resp = e.handle_command(Command::CaptureDiagnose).unwrap();
        match resp {
            Response::CaptureDiagnosis(d) => {
                assert!(!d.healthy);
                assert!(d.issues.iter().any(|i| i.contains("Capture process exited")));
            }
            other => panic!("expected CaptureDiagnosis, got {other:?}"),
        }
        // Fix All resets the stale session and the report becomes healthy.
        let resp2 = e.handle_command(Command::CaptureFix).unwrap();
        match resp2 {
            Response::CaptureDiagnosis(d) => {
                assert!(d.healthy);
                assert_eq!(e.snapshot().capture_session.state, CaptureRunState::Idle);
            }
            other => panic!("expected CaptureDiagnosis, got {other:?}"),
        }
    }
}
