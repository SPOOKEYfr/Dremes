//! Dremes Engine core library.
//!
//! The Engine is the single owner of the system state and runs in the
//! background. It coordinates Waydroid, ADB, capture, and automatic recovery.
//! The GUI is a separate process that sends intents and displays state.
//!
//! This crate is built as a library so the state machine is unit-testable,
//! and as a binary (`dremes-engine`) so it can run as a background daemon
//! independently of the GUI.

pub mod engine;
pub mod ipc;
pub mod process_manager;
pub mod recovery;
pub mod registry;

pub use engine::Engine;
pub use recovery::RecoveryWorker;
