//! Dremes error handling.
//!
//! Errors are structured so the UI can show an understandable message,
//! the affected component, a suggested action, and expandable technical
//! details instead of a bare "exit code 1".

use serde::Serialize;
use thiserror::Error;

/// The application-facing error type used across Dremes.
#[derive(Debug, Error, Serialize, Clone)]
#[serde(tag = "kind", content = "message")]
pub enum DremesError {
    /// A generic/manual error carrying a human-readable message.
    #[error("{0}")]
    Message(String),

    /// A system call / command failed.
    #[error("Command failed: {command}: {detail}")]
    Command {
        /// The command that was attempted.
        command: String,
        /// A technical detail (e.g. exit status).
        detail: String,
    },

    /// Waydroid-specific failure with a suggested action.
    #[error("Waydroid: {message}. {suggestion}")]
    Waydroid {
        message: String,
        suggestion: String,
    },

    /// ADB-specific failure.
    #[error("ADB: {0}")]
    Adb(String),

    /// Capture / V4L2-specific failure.
    #[error("Capture: {0}")]
    Capture(String),

    /// Video4Linux device failure.
    #[error("V4L2: {0}")]
    V4l2(String),

    /// A required executable was not found on the system.
    #[error("Required executable not found: {0}. Check that it is installed.")]
    NotFound(String),
}

impl DremesError {
    /// Construct a human-facing command error.
    pub fn command(command: impl Into<String>, detail: impl Into<String>) -> Self {
        DremesError::Command {
            command: command.into(),
            detail: detail.into(),
        }
    }

    /// A convenience for a plain message.
    pub fn msg(m: impl Into<String>) -> Self {
        DremesError::Message(m.into())
    }
}

/// Result alias used throughout the codebase.
pub type Result<T> = std::result::Result<T, DremesError>;

/// An expandable diagnostic error report for the UI.
#[derive(Debug, Serialize, Clone)]
pub struct ErrorReport {
    /// The component that failed.
    pub component: String,
    /// Human-readable description.
    pub message: String,
    /// A suggested next action for the user.
    pub suggestion: String,
    /// Optional technical detail, expandable in the UI.
    pub detail: Option<String>,
}

impl From<DremesError> for ErrorReport {
    fn from(e: DremesError) -> Self {
        let (message, hint) = match &e {
            DremesError::Message(m) => (m.clone(), None),
            DremesError::Command { command, detail } => (
                format!("Command `{command}` failed."),
                Some(detail.clone()),
            ),
            DremesError::Waydroid { message, suggestion } => (
                message.clone(),
                Some(suggestion.clone()),
            ),
            DremesError::Adb(m) => (m.clone(), None),
            DremesError::Capture(m) => (m.clone(), None),
            DremesError::V4l2(m) => (m.clone(), None),
            DremesError::NotFound(name) => (
                format!("Required program `{name}` was not found."),
                Some("Install it and restart Dremes.".into()),
            ),
        };
        ErrorReport {
            component: "system".into(),
            message: hint.as_deref().unwrap_or(&message).to_string(),
            suggestion: String::new(),
            detail: Some(message),
        }
    }
}
