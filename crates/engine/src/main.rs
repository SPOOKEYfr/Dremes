//! Dremes Engine daemon entry point.
//!
//! Listens for JSON-line IPC commands from the GUI over a Unix domain socket
//! and manages the system state. Runs independently of the GUI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use dremes_core::error::Result as CoreResult;
use dremes_engine::ipc::{Ack, Command, Response};
use dremes_engine::Engine;

fn socket_path() -> PathBuf {
    dremes_core::settings::Settings::config_dir().join("engine.sock")
}

/// Process one JSON IPC line and return the response.
fn handle_line(engine: &Engine, line: &str) -> String {
    let cmd: Command = match serde_json::from_str(line) {
        Ok(c) => c,
        Err(e) => {
            return serde_json::to_string(&Response::Ack(Ack {
                ok: false,
                error: Some(format!("invalid command: {e}")),
            }))
            .unwrap_or_else(|_| String::new());
        }
    };
    match engine.handle_command(cmd) {
        Ok(resp) => serde_json::to_string(&resp).unwrap_or_else(|_| String::new()),
        Err(e) => serde_json::to_string(&Response::Ack(Ack {
            ok: false,
            error: Some(e.to_string()),
        }))
        .unwrap_or_else(|_| String::new()),
    }
}

fn main() -> CoreResult<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Dremes Engine {} starting", dremes_core::version::version_string());

    // Ensure config dir exists.
    let cfg = dremes_core::settings::Settings::config_dir();
    std::fs::create_dir_all(&cfg).ok();

    let engine = Engine::default();
    log::info!("Engine initialized. Master switch status: {:?}", engine.snapshot().engine);

    // Start the automatic-recovery worker. It is a no-op while the master
    // switch is OFF (the default), so it never acts unless the user asks.
    // Bound as `_recovery` to keep it alive for the daemon's lifetime.
    let poll_secs = dremes_core::settings::Settings::load().capture.retry_delay_secs.max(2);
    let _recovery = dremes_engine::RecoveryWorker::start(engine.clone(), poll_secs);
    log::info!("Recovery worker started (poll {}s)", poll_secs);

    // On Linux, use a Unix domain socket.
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;
        let sock = socket_path();
        if sock.exists() {
            let _ = std::fs::remove_file(&sock);
        }
        let listener = UnixListener::bind(&sock).map_err(|e| {
            dremes_core::error::DremesError::msg(format!("cannot bind {sock:?}: {e}"))
        })?;
        log::info!("Engine listening on {:?}", sock);

        let mut conns: u64 = 0;
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    conns += 1;
                    log::debug!("accepted IPC connection #{}", conns);
                    let reader = BufReader::new(stream.try_clone().unwrap());
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(_) => break,
                        };
                        let resp = handle_line(&engine, &line);
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();
                    }
                }
                Err(_) => continue,
            }
        }
    }

    // Non-unix platforms: keep the process alive until signaled.
    #[cfg(not(unix))]
    {
        log::info!("Engine running (GUI attaches over platform IPC)");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    Ok(())
}