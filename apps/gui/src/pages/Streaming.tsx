import { useState } from "react";
import { Card, StatusPill, Button } from "../components/ui";
import { useAppState, engineCommand } from "../state";

export default function Streaming() {
  const { state } = useAppState();
  const [busy, setBusy] = useState<null | string>(null);
  const capturing = state.capture === "Running" || state.capture === "Starting";
  const engineOn = state.engine_enabled;

  const run = async (cmd: "StartStreaming" | "StopStreaming" | "ReconnectToObs" | "OpenObs" | "CloseObs" | "StreamBoost") => {
    setBusy(cmd);
    try { await engineCommand(cmd); } finally { setBusy(null); }
  };

  const obsStatus = state.obs_status || "UNKNOWN";
  const obsOk = obsStatus === "RUNNING";
  const obsWarn = obsStatus === "NOT RUNNING" || obsStatus === "UNKNOWN";

  return (
    <div className="page">
      <header className="page-head">
        <h1>Streaming</h1>
        <p>Direct Waydroid display to OBS via /dev/video10.</p>
        {!engineOn && <div className="engine-gate">Engine is OFF — enable it to control streaming.</div>}
      </header>
      <Card title="Pipeline">
        <div className="pipeline">
          <span>Waydroid</span>
          <span className="arrow">→</span>
          <span>scrcpy (headless)</span>
          <span className="arrow">→</span>
          <span>/dev/video10</span>
          <span className="arrow">→</span>
          <span>OBS V4L2</span>
        </div>
      </Card>
      <div className="grid">
        <Card title="Streaming Controls">
          <Button variant="primary" disabled={busy !== null || capturing || !engineOn} onClick={() => run("StartStreaming")}>
            {busy === "StartStreaming" ? "STARTING…" : "START STREAMING"}
          </Button>
          <div className="spacer" />
          <Button variant="danger" disabled={busy !== null || !capturing || !engineOn} onClick={() => run("StopStreaming")}>
            STOP STREAMING
          </Button>
          <div className="spacer" />
          <Button variant="ghost" disabled={busy !== null || !engineOn} onClick={() => run("ReconnectToObs")}>
            RECONNECT TO OBS
          </Button>
          <div className="spacer" />
          <Button variant="ghost" disabled={busy !== null} onClick={() => run("OpenObs")}>
            OPEN OBS
          </Button>
          <div className="spacer" />
          <Button variant="danger" disabled={busy !== null || !obsOk} onClick={() => run("CloseObs")}>
            CLOSE OBS
          </Button>
        </Card>
        <Card title="Status">
          <div className="status-row">
            <span>Capture</span>
            <StatusPill label={state.capture.toUpperCase()} status={capturing ? "running" : "stopped"} />
          </div>
          <div className="status-row">
            <span>ADB</span>
            <StatusPill label={state.adb_connected ? "CONNECTED" : "DISCONNECTED"} status={state.adb_connected ? "running" : "stopped"} />
          </div>
          <div className="status-row">
            <span>Waydroid</span>
            <StatusPill label={state.waydroid.toUpperCase()} status={state.waydroid === "Running" ? "running" : "stopped"} />
          </div>
          <div className="status-row">
            <span>OBS</span>
            <StatusPill label={obsStatus} status={obsOk ? "running" : obsWarn ? "warning" : "bad"} />
          </div>
          <div className="status-row">
            <span>Reconnects</span>
            <strong>{state.reconnect_count}</strong>
          </div>
          <div className="status-row">
            <span>Resolution</span>
            <strong>
              {state.capture_info.width > 0 ? `${state.capture_info.width}×${state.capture_info.height}` : "—"}
            </strong>
          </div>
          {state.stream_boost_note && (
            <div className="status-row">
              <span>Stream Boost</span>
              <strong>{state.stream_boost_note}</strong>
            </div>
          )}
          {state.v4l2_detail && (
            <div className="status-row">
              <span>V4L2 Detail</span>
              <strong className="mono">{state.v4l2_detail}</strong>
            </div>
          )}
        </Card>
        <Card title="Boosts">
          <div className="boost-row">
            <label className="switch">
              <input
                type="checkbox"
                checked={state.stream_boost}
                onChange={(e) =>
                  void engineCommand("StreamBoost", {
                    play: e.target.checked ? "boost_on" : "boost_off",
                  })
                }
              />
              <span className="slider" />
            </label>
            <span className="switch-label">Stream Boost</span>
          </div>
          {state.stream_boost_note && <div className="status-row"><span>Effect</span><strong>{state.stream_boost_note}</strong></div>}
        </Card>
      </div>
    </div>
  );
}