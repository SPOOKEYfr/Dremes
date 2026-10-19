//! Process registry.
//!
//! Every process Dremes spawns is recorded here with a role. The Engine may
//! stop ONLY processes in this registry (or processes it can positively
//! identify as stale Dremes components). It never blindly kills all scrcpy /
//! adb / waydroid / ffmpeg processes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use dremes_core::process::{OwnedProcess, ProcessRole};

/// Central registry of Dremes-owned processes.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProcessRegistry {
    /// keyed by pid.
    processes: HashMap<u32, OwnedProcess>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a freshly spawned owned process.
    pub fn register(&mut self, proc: OwnedProcess) {
        self.processes.insert(proc.pid, proc);
    }

    /// Unregister (a process has exited or was stopped).
    pub fn unregister(&mut self, pid: u32) {
        self.processes.remove(&pid);
    }

    /// All processes with a given role.
    pub fn by_role(&self, role: ProcessRole) -> Vec<&OwnedProcess> {
        self.processes.values().filter(|p| p.role == role).collect()
    }

    /// Look up a single process by pid.
    pub fn lookup(&mut self, pid: u32) -> Option<&mut OwnedProcess> {
        self.processes.get_mut(&pid)
    }

    /// Whether any process with a role is currently tracked.
    pub fn has_role(&self, role: ProcessRole) -> bool {
        !self.by_role(role).is_empty()
    }

    /// The set of PIDs we own (used for safe cleanup).
    pub fn owned_pids(&self) -> Vec<u32> {
        self.processes.keys().copied().collect()
    }

    /// Purge entries for processes that have exited (alive_marker false).
    pub fn purge_stale(&mut self) {
        self.processes.retain(|_, p| p.alive_marker);
    }

    /// A helper to mark all as potentially stale (pre-reactive-check).
    pub fn mark_all_stale(&mut self) {
        for p in self.processes.values_mut() {
            p.alive_marker = false;
        }
    }

    /// Refresh liveness using a provided predicate (e.g. checks /proc).
    pub fn refresh<F: Fn(u32) -> bool>(&mut self, alive: F) {
        self.processes.retain(|pid, p| {
            let still_alive = alive(*pid);
            p.alive_marker = still_alive;
            still_alive
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_and_finds_by_role() {
        let mut reg = ProcessRegistry::new();
        reg.register(OwnedProcess::new(100, ProcessRole::Capture, 0));
        reg.register(OwnedProcess::new(200, ProcessRole::Waydroid, 0));
        assert_eq!(reg.by_role(ProcessRole::Capture).len(), 1);
        assert_eq!(reg.by_role(ProcessRole::Waydroid).len(), 1);
        assert!(!reg.has_role(ProcessRole::AdbServer));
    }

    #[test]
    fn only_unregisters_its_own() {
        let mut reg = ProcessRegistry::new();
        reg.register(OwnedProcess::new(100, ProcessRole::Capture, 0));
        reg.unregister(100);
        assert!(!reg.has_role(ProcessRole::Capture));
    }

    #[test]
    fn refresh_drops_dead_processes() {
        let mut reg = ProcessRegistry::new();
        reg.register(OwnedProcess::new(100, ProcessRole::Capture, 0));
        reg.register(OwnedProcess::new(200, ProcessRole::Capture, 0));
        // pid 100 no longer alive.
        reg.refresh(|pid| pid != 100);
        let remaining = reg.by_role(ProcessRole::Capture);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].pid, 200);
    }

    #[test]
    fn multiple_capture_writers_can_be_detected() {
        let mut reg = ProcessRegistry::new();
        reg.register(OwnedProcess::new(100, ProcessRole::Capture, 0));
        reg.register(OwnedProcess::new(101, ProcessRole::Capture, 0));
        // Invariant: exactly one writer is enforced in Engine; here we verify
        // the detection path sees both so it can clean up extras.
        assert_eq!(reg.by_role(ProcessRole::Capture).len(), 2);
    }
}
