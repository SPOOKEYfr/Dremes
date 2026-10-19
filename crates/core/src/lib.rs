//! Dremes shared core.
//!
//! This crate contains everything shared between the GUI and the Engine:
//! - State definitions and the state machine (the single source of truth)
//! - Error types
//! - Process ownership model
//! - Version information

pub mod capture;
pub mod error;
pub mod platform;
pub mod process;
pub mod settings;
pub mod state;
pub mod version;
