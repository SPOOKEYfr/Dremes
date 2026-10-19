//! Safe command execution helpers.
//!
//! All external commands run through here. Arguments are always passed as
//! separate OS arguments — never joined into a shell string — so arbitrary
//! user input cannot be interpreted as shell syntax.

use std::process::{Command, Stdio};

use dremes_core::error::{DremesError, Result};

/// Run a command, capturing stdout, without a shell.
pub fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DremesError::NotFound(cmd.into())
            } else {
                DremesError::Command {
                    command: cmd.into(),
                    detail: e.to_string(),
                }
            }
        })?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(DremesError::Command {
            command: format!("{cmd} {}", args.join(" ")),
            detail: format!(
                "exit {:?}: {}",
                out.status.code(),
                if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }
            ),
        });
    }
    Ok(stdout)
}

/// Run a command with a real wall-clock timeout, capturing stdout.
///
/// Uses the system `timeout` wrapper so a hung child (e.g. adb) cannot block
/// the Engine forever. Arguments are still passed separately, never shell-joined.
pub fn run_timeout(cmd: &str, args: &[&str], secs: u64) -> Result<String> {
    let mut wrapped: Vec<String> = vec!["--kill-after=2".into(), secs.to_string(), cmd.into()];
    wrapped.extend(args.iter().map(|s| s.to_string()));
    let wrapped_args: Vec<&str> = wrapped.iter().map(|s| s.as_str()).collect();

    let out = Command::new("timeout")
        .args(&wrapped_args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DremesError::NotFound("timeout".into())
            } else {
                DremesError::Command {
                    command: cmd.into(),
                    detail: e.to_string(),
                }
            }
        })?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        let code = out.status.code();
        // 124 = timed out
        if let Some(124) = code {
            return Err(DremesError::Command {
                command: format!("{cmd} {}", args.join(" ")),
                detail: format!("command timed out after {secs}s"),
            });
        }
        return Err(DremesError::Command {
            command: format!("{cmd} {}", args.join(" ")),
            detail: format!(
                "exit {:?}: {}",
                code,
                if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() }
            ),
        });
    }
    Ok(stdout)
}

/// Check whether an executable exists on PATH.
///
/// `cmd` is an internal, hardcoded program name (never user input), so
/// formatting it into the `sh -c` string is safe.
pub fn which(cmd: &str) -> bool {
    let script = format!("command -v {cmd}");
    Command::new("sh")
        .args(["-c", script.as_str()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn a long-lived process fully detached: stdin/out/err are redirected to
/// null and the parent returns immediately without waiting or ever signalling
/// the child. Used for daemon-like processes (e.g. `waydroid session start`)
/// that must keep running for the system to function. The child is not tracked
/// as an OwnedProcess — it is an external system service, not one Dremes owns.
pub fn spawn_detached(cmd: &str, args: &[&str]) -> Result<()> {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                DremesError::NotFound(cmd.into())
            } else {
                DremesError::Command {
                    command: cmd.into(),
                    detail: e.to_string(),
                }
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_executes_simple_command() {
        let out = run("echo", &["dremes"]).unwrap();
        assert!(out.trim() == "dremes");
    }

    #[test]
    fn run_reports_missing_program() {
        match run("definitely-not-a-real-program-xyz", &[]) {
            Err(DremesError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn which_finds_existing() {
        assert!(which("echo"));
        assert!(!which("definitely-not-a-real-program-xyz"));
    }
}
