//! Automatic recovery worker.
//!
//! While the Engine master switch is ON, a background worker periodically
//! checks the health of the system and attempts to recover from failures by
//! re-issuing the appropriate intent command through the existing state
//! machine (which guarantees at most one capture writer and clean ownership):
//!
//! - **Streaming mode**: if the single capture writer dies (process gone or a
//!   writer was expected but has no handle), re-issue `ReconnectToObs` to
//!   restart the pipeline to OBS. If Waydroid also dropped, that command
//!   restores it too.
//! - **Gaming mode**: if Waydroid was requested but is not running, re-issue
//!   `GamingMode` to bring it back with capture fully OFF.
//!
//! The worker is conservative and ownership-aware:
//! - It only acts when the user asked for the relevant mode (never idle).
//! - It respects per-component retry limits from `Settings`.
//! - Every (re)connect increments the session `reconnect_count` and is logged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use dremes_core::settings::Settings;
use dremes_core::state::{Mode, StreamingState, WaydroidState};

use crate::engine::Engine;
use crate::ipc::Command;

/// True while a component is mid-transition; recovery must never act on these
/// states (it would thrash a start/stop in-flight).
fn waydroid_in_transition(w: WaydroidState) -> bool {
    matches!(w, WaydroidState::Starting | WaydroidState::Stopping)
}

/// Spawns and owns the recovery worker thread.
pub struct RecoveryWorker {
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RecoveryWorker {
    /// Start the worker. It polls the engine until `stop()` is called.
    pub fn start(engine: Engine, poll_secs: u64) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        let handle = std::thread::spawn(move || {
            loop {
                if c.load(Ordering::Relaxed) {
                    break;
                }
                engine.recovery_pass();
                std::thread::sleep(Duration::from_secs(poll_secs.max(1)));
            }
        });
        Self {
            cancel,
            handle: Some(handle),
        }
    }

    /// Signal the worker to stop and join its thread.
    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for RecoveryWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Engine {
    /// Run a single recovery/heartbeat pass.
    ///
    /// Safe to call from any thread (e.g. the worker or tests). It holds the
    /// engine lock only briefly to read state, never across backend calls.
    pub fn recovery_pass(&self) {
        let s = self.snapshot();

        // Recovery only makes sense while the user asked for an active mode.
        if !s.engine_enabled {
            return;
        }

        // Determine what should be running based on the requested mode.
        let mode = s.mode;
        match mode {
            Mode::Idle => return,
            Mode::Gaming => {
                // Healthy: Waydroid running. Reset the retry episode.
                if s.waydroid == WaydroidState::Running {
                    self.reset_recovery_episode();
                    return;
                }
                // Never act while a start/stop is in flight.
                if waydroid_in_transition(s.waydroid) {
                    return;
                }
                // Waydroid is Stopped/Error: attempt a bounded recovery.
                self.try_recover(
                    "waydroid",
                    Settings::load().waydroid_retry_limit,
                    Command::GamingMode,
                    "restart Waydroid (Gaming Mode)",
                );
            }
            Mode::Streaming => {
                // Healthy: Waydroid running, writer alive, streaming Active.
                let capture_gone = self.capture_handle_alive() == Some(false);
                let streaming_lost = s.streaming != StreamingState::Active;
                let healthy = s.waydroid == WaydroidState::Running && !capture_gone && !streaming_lost;
                if healthy {
                    self.reset_recovery_episode();
                    return;
                }
                if waydroid_in_transition(s.waydroid) {
                    return;
                }
                // Waydroid missing: restore everything.
                if s.waydroid != WaydroidState::Running {
                    self.try_recover(
                        "waydroid",
                        Settings::load().waydroid_retry_limit,
                        Command::ReconnectToObs,
                        "restart Waydroid and re-stream",
                    );
                    return;
                }
                // Waydroid is up but the writer/stream is lost: reconnect.
                if capture_gone || streaming_lost {
                    self.try_recover(
                        "capture",
                        Settings::load().capture.max_retries,
                        Command::ReconnectToObs,
                        "restart capture writer",
                    );
                }
            }
        }
    }

    /// Bounded recovery: only act if we haven't exhausted the retry budget and
    /// the cooldown has elapsed. Rate limits and per-episode budgets make the
    /// worker resilient to downstream components that are slow to reach a
    /// healthy state, so it can never re-issue intents in a tight loop.
    fn try_recover(
        &self,
        component: &str,
        limit: u32,
        cmd: Command,
        action: &str,
    ) {
        if !self.recovery_ready(limit) {
            if self.recovery_episode_exhausted(limit) {
                self.record_log(
                    "recovery",
                    "error",
                    &format!("{component} recovery budget exhausted after {limit} attempts; pausing — manual action advised"),
                );
            }
            // Cooldown still active or budget used; do nothing this pass.
            return;
        }
        self.record_log(
            "recovery",
            "warn",
            &format!("{component} unhealthy; attempting to {action}"),
        );
        self.mark_recovery_attempt();
        let _ = self.handle_command(cmd);
    }
}
