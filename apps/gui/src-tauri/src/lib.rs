//! Dremes GUI (Tauri) Rust entry.
//!
//! The GUI is a thin shell: it renders the frontend and proxies intents to the
//! Engine daemon via IPC. It does NOT itself manage Waydroid/ADB/capture — that
//! is the Engine's responsibility, so the GUI can close while the Engine keeps
//! streaming.
//!
//! The Engine is an opt-in user systemd unit. It is NOT started at install or
//! login, and the GUI does not spawn it on launch. The one exception: pressing
//! "Engine ON" starts the opt-in Engine on demand if it isn't running yet, so a
//! freshly installed app works on the first click. All other commands just
//! connect to the Engine's Unix socket and mirror the IPC protocol
//! (newline-delimited JSON of the `Command`/`Response` enums).

use dremes_core::settings::Settings;
use std::io::{BufRead, BufReader, Write};

/// Send one JSON-line command to the Engine daemon over its Unix socket and
/// return the raw response line. Returns an error if the Engine isn't running.
fn request_engine(payload: &str) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let path = Settings::config_dir().join("engine.sock");
        let mut stream = UnixStream::connect(&path).map_err(|e| {
            format!("Dremes Engine is not running (could not connect to {path:?}: {e})")
        })?;
        let _ = stream.write_all(payload.as_bytes());
        let _ = stream.write_all(b"\n");
        let _ = stream.flush();
        let reader = BufReader::new(stream);
        // Read the engine's single response line.
        let mut resp: Option<String> = None;
        for line in reader.lines() {
            match line {
                Ok(l) => { resp = Some(l); break }
                Err(_) => break,
            }
        }
        match resp {
            Some(r) => Ok(r),
            None => Err("Engine closed the connection without a response.".into()),
        }
    }
    #[cfg(not(unix))]
    {
        Err("Engine IPC is only available on Unix platforms.")
    }
}

#[tauri::command]
fn dremes_version() -> String {
    dremes_core::version::version_string()
}

/// Send an intent/query to the Engine. `command` is a bare Command variant
/// name, e.g. "GetState". Returns the engine's `Response` envelope as JSON.
///
/// "EngineOn" is special: if the Engine daemon isn't running yet, it starts the
/// opt-in `dremes-engine.service` user unit first and waits for it to come up,
/// so a fresh install works on first use.
#[tauri::command]
fn engine_command(command: String) -> Result<serde_json::Value, String> {
    engine_command_inner(&command, None)
}

/// Payload-bearing variant of `engine_command`. The GUI calls this for commands
/// whose variants carry a struct, e.g. `StartCapture(CaptureRequest)`. `payload`
/// is the serde-serializable inner value of the variant.
#[tauri::command]
fn engine_command_with(
    command: String,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    engine_command_inner(&command, Some(payload))
}

/// Shared implementation: builds `"<Command>"` or `{"Command": payload}` and
/// forwards to the Engine socket, optionally starting the daemon for EngineOn.
fn engine_command_inner(
    command: &str,
    payload: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let line = match payload {
        Some(val) => serde_json::json!({ command: val }).to_string(),
        None => serde_json::to_string(command).map_err(|e| format!("bad command: {e}"))?,
    };

    let attempt = || request_engine(&line);

    if command == "EngineOn" {
        if attempt().is_err() {
            start_engine_unit();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
            while std::time::Instant::now() < deadline {
                if let Ok(resp) = attempt() {
                    return serde_json::from_str::<serde_json::Value>(&resp)
                        .map_err(|e| format!("bad engine response: {e}"));
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }

    let resp = attempt()?;
    serde_json::from_str::<serde_json::Value>(&resp)
        .map_err(|e| format!("bad engine response: {e}"))
}

/// Start the opt-in Engine user unit. Best-effort: if it's already running or
/// `systemctl` isn't available, this is a no-op.
#[cfg(unix)]
fn start_engine_unit() {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", "dremes-engine.service"])
        .status();
}

#[cfg(not(unix))]
fn start_engine_unit() {}

/// Ping the Engine for liveness; true if the daemon is reachable.
#[tauri::command]
fn engine_health() -> bool {
    request_engine("\"Ping\"").ok().is_some()
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

/// Write a UTF-8 text file to `path` (used by the Logs "Export" save dialog).
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("Failed to write {path}: {e}"))
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())
}

#[tauri::command]
fn run_diagnostics() -> Vec<serde_json::Value> {
    // On Linux, run the real diagnostics; on other platforms, a graceful notice.
    #[cfg(target_os = "linux")]
    {
        let items = dremes_linux_backend::diagnostics::run();
        items
            .into_iter()
            .map(|i| {
                serde_json::json!({
                    "key": i.key,
                    "label": i.label,
                    "status": i.status,
                    "message": i.message,
                })
            })
            .collect()
    }
    #[cfg(not(target_os = "linux"))]
    {
        vec![serde_json::json!({
            "key": "platform",
            "label": "Platform",
            "status": "warning",
            "message": "Waydroid management is unavailable on this platform."
        })]
    }
}

/// Run Dremes' permanent capture repair script via `pkexec` (root; shows the
/// session's polkit password dialog). Returns the script output for display.
///
/// The script only touches capture infrastructure (v4l2loopback persistence and
/// loading, OBS plugin check) — never the running capture or Waydroid state.
#[tauri::command]
fn run_root_fix() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let script = "/usr/lib/dremes-engine/fix-capture-root.sh";
        if !std::path::Path::new(script).exists() {
            return Err(format!(
                "Permanent-fix helper missing at {script}. Reinstall the dremes package."
            ));
        }
        let out = std::process::Command::new("pkexec")
            .args(["bash", script])
            .output()
            .map_err(|e| format!("Could not launch pkexec (is polkit installed?): {e}"))?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.stderr.is_empty() {
            text.push_str(&String::from_utf8_lossy(&out.stderr));
        }
        if !out.status.success() {
            return Err(format!(
                "Permanent fix did not complete cleanly (exit {}):\n{}",
                out.status.code().unwrap_or(-1),
                text
            ));
        }
        Ok(text)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("Capture repairs are only available on Linux.".into())
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            dremes_version,
            engine_command,
            engine_command_with,
            engine_health,
            get_settings,
            save_settings,
            write_text_file,
            run_diagnostics,
            run_root_fix
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dremes");
}