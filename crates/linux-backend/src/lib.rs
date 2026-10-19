//! Dremes Linux backend.
//!
//! Implements the `PlatformBackend` trait for Linux by talking to:
//! - Waydroid (CLI + session)
//! - ADB (dynamic endpoint discovery)
//! - scrcpy (direct V4L2 sink capture)
//! - V4L2 (device validation + frame verification)
//!
//! The invariant enforced here: at most ONE capture writer owns the V4L2
//! device at any time, and Dremes fully owns its lifecycle.

pub mod adb;
pub mod apps;
pub mod capture;
pub mod capture_src;
pub mod command;
pub mod diagnostics;
pub mod backend;
pub mod obs;
pub mod resources;
pub mod v4l2;
pub mod waydroid;
pub mod wifi;

pub use backend::LinuxBackend;
