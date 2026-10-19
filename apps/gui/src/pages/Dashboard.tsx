import { useState } from "react";
import { Card, StatusPill, Button } from "../components/ui";
import { useAppState, engineCommand } from "../state";

export default function Dashboard() {
  const { state, resources, connected } = useAppState();
  const [busy, setBusy] = useState<string | null>(null);

  const engineOn = state.engine_enabled;
  const waydroidOn = state.waydroid === "Running" || state.waydroid === "Starting";

  const run = async (cmd: "EngineOn" | "EngineOff" | "GamingMode" | "StartStreaming" | "StopStreaming" | "StopWaydroid" | "ReconnectToObs" | "OpenObs" | "CloseObs" | "StreamBoost" | "GameBoost" | "Super") => {
    setBusy(cmd);
    await engineCommand(cmd);
    setBusy(null);
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>Dashboard</h1>
        <p>Waydroid &amp; Streaming Control Center</p>
        {!connected && <span className="pill warn">ENGINE OFFLINE</span>}
      </header>

      {/* Engine master switch */}
      <Card>
        <div className="engine-row">
          <div>
            <div className="card-kicker">DREMES ENGINE</div>
            <div className={`engine-state ${engineOn ? "on" : "off"}`}>
              {engineOn ? "ENGINE RUNNING" : "ENGINE OFF"}
            </div>
          </div>
          <div className="switch-wrap">
            <label className="switch">
              <input
                type="checkbox"
                checked={engineOn}
                onChange={() => run(engineOn ? "EngineOff" : "EngineOn")}
              />
              <span className="slider" />
            </label>
            <span className="switch-label">{engineOn ? "ON" : "OFF"}</span>
          </div>
        </div>
      </Card>

      <div className="grid">
        {/* SUPER focus mode */}
        <Card>
          <div className="super-hero">
            <div className="super-copy">
              <div className="card-kicker">FOCUS MODE</div>
              <div className="super-title">SUPER</div>
              <p className="hint">
                Closes your background applications so streaming &amp; gaming get every
                CPU cycle. System apps, terminals, OBS, Waydroid and Dremes are always kept.
              </p>
            </div>
            <button
              className="btn-super"
              disabled={busy !== null || !engineOn}
              onClick={() => run("Super")}
            >
              {busy === "Super" ? "CLEARING…" : "SUPER"}
            </button>
          </div>
          {state.super_report && (
            <div className="status-row super-report">
              <span>Last Report</span>
              <strong>{state.super_report}</strong>
            </div>
          )}
        </Card>

        {/* System status */}
        <Card title="System Status">
          <div className="status-row">
            <span>CPU</span>
            <strong>{resources.cpu_total > 0 ? `${resources.cpu_total.toFixed(0)}%` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>RAM</span>
            <strong>
              {resources.ram_used_mb > 0 ? `${(resources.ram_used_mb / 1024).toFixed(1)} GiB` : "—"}
            </strong>
          </div>
          <div className="status-row">
            <span>Waydroid</span>
            <StatusPill
              label={state.waydroid.toUpperCase()}
              status={waydroidOn ? "running" : "stopped"}
            />
          </div>
          <div className="status-row">
            <span>ADB</span>
            <StatusPill
              label={state.adb_connected ? "CONNECTED" : "OFF"}
              status={state.adb_connected ? "running" : "stopped"}
            />
          </div>
          <div className="status-row">
            <span>Capture</span>
            <StatusPill
              label={state.capture.toUpperCase()}
              status={state.capture === "Running" || state.capture === "Starting" ? "running" : "stopped"}
            />
          </div>
          <div className="status-row">
            <span>OBS</span>
            <StatusPill
              label={state.obs_status || "UNKNOWN"}
              status={state.obs_status === "RUNNING" ? "running" : state.obs_status === "NOT INSTALLED" ? "bad" : "warning"}
            />
          </div>
        </Card>

        {/* Mode */}
        <Card title="Mode">
          {!engineOn && <div className="engine-gate">Engine is OFF — enable it to switch modes.</div>}
          <div className="mode-actions">
            <Button variant={state.mode === "Gaming" ? "primary" : "ghost"} onClick={() => run("GamingMode")} disabled={busy !== null || !engineOn}>
              {busy === "GamingMode" ? "Starting…" : "Gaming Mode"}
            </Button>
            <Button variant={state.mode === "Streaming" ? "primary" : "ghost"} onClick={() => run("StartStreaming")} disabled={busy !== null || !engineOn}>
              {busy === "StartStreaming" ? "Starting…" : "Streaming Mode"}
            </Button>
          </div>
          {state.mode !== "Idle" && state.mode !== undefined && (
            <div className="mode-active">
              <span className="pulse-dot" /> {state.mode === "Gaming" ? "Gaming Mode" : "Streaming Mode"}{" "}
              <strong>ACTIVE</strong>
            </div>
          )}
        </Card>

        {/* Waydroid */}
        <Card title="Waydroid">
          <div className="status-row">
            <span>Status</span>
            <StatusPill label={state.waydroid.toUpperCase()} status={waydroidOn ? "running" : "stopped"} />
          </div>
          <div className="status-row">
            <span>ADB Endpoint</span>
            <strong>{state.adb_endpoint || "—"}</strong>
          </div>
          <div className="inline-actions">
            <Button variant="primary" onClick={() => run("GamingMode")} disabled={waydroidOn || !engineOn}>
              START WAYDROID
            </Button>
            <Button variant="danger" onClick={() => run("StopWaydroid")} disabled={!waydroidOn || !engineOn}>
              STOP WAYDROID
            </Button>
          </div>
        </Card>

        {/* Streaming */}
        <Card title="Streaming">
          <div className="status-row">
            <span>Capture</span>
            <StatusPill
              label={state.capture.toUpperCase()}
              status={state.capture === "Running" || state.capture === "Starting" ? "running" : "stopped"}
            />
          </div>
          <div className="status-row">
            <span>Resolution</span>
            <strong>
              {state.capture_info.width > 0
                ? `${state.capture_info.width}×${state.capture_info.height}`
                : "—"}
            </strong>
          </div>
          <div className="status-row">
            <span>FPS</span>
            <strong>{state.capture_info.fps > 0 ? `${state.capture_info.fps.toFixed(0)}` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>Device</span>
            <strong>{state.capture_info.device || "/dev/video10"}</strong>
          </div>
          <div className="inline-actions">
            <Button
              variant="primary"
              onClick={() => run("StartStreaming")}
              disabled={state.capture === "Running" || state.capture === "Starting" || busy !== null || !engineOn}
            >
              START STREAMING
            </Button>
            <Button
              variant="danger"
              onClick={() => run("StopStreaming")}
              disabled={state.capture === "Off" || state.streaming === "Off" || !engineOn}
            >
              STOP STREAMING
            </Button>
            <Button variant="ghost" onClick={() => run("ReconnectToObs")} disabled={!waydroidOn || !engineOn}>
              RECONNECT TO OBS
            </Button>
            <Button variant="ghost" onClick={() => run("OpenObs")}>
              OPEN OBS
            </Button>
            <Button variant="danger" onClick={() => run("CloseObs")} disabled={state.obs_status !== "RUNNING"}>
              CLOSE OBS
            </Button>
          </div>
          <div className="boost-row">
            <label className="switch">
              <input type="checkbox" checked={state.stream_boost} onChange={(e) => void engineCommand("StreamBoost", { play: e.target.checked ? "boost_on" : "boost_off" })} />
              <span className="slider" />
            </label>
            <span className="switch-label">Stream Boost</span>
            <label className="switch">
              <input type="checkbox" checked={state.game_boost} onChange={(e) => void engineCommand("GameBoost", { play: e.target.checked ? "boost_on" : "boost_off" })} />
              <span className="slider" />
            </label>
            <span className="switch-label">Game Boost</span>
          </div>
          {state.stream_boost_note && <div className="status-row"><span>Stream Boost</span><strong>{state.stream_boost_note}</strong></div>}
          {state.game_boost_note && <div className="status-row"><span>Game Boost</span><strong>{state.game_boost_note}</strong></div>}
        </Card>

        {/* Resource usage */}
        <Card title="Resource Usage">
          <div className="status-row">
            <span>CPU</span>
            <strong>{resources.cpu_total > 0 ? `${resources.cpu_total.toFixed(0)}%` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>RAM</span>
            <strong>
              {resources.ram_used_mb > 0 ? `${(resources.ram_used_mb / 1024).toFixed(1)} GiB` : "—"}
            </strong>
          </div>
          <div className="status-row">
            <span>Waydroid</span>
            <strong>{resources.waydroid_cpu > 0 ? `${resources.waydroid_cpu.toFixed(0)}%` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>Capture</span>
            <strong>{resources.capture_cpu > 0 ? `${resources.capture_cpu.toFixed(0)}%` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>Dremes</span>
            <strong>{resources.engine_cpu > 0 ? `${resources.engine_cpu.toFixed(0)}%` : "—"}</strong>
          </div>
        </Card>

        {/* Recent events */}
        <Card title="Recent Events">
          <div className="events">
            {!connected ? (
              <div className="event muted">Engine offline — start it to begin streaming.</div>
            ) : (
              <div className="event">
                <span className="pulse-dot" /> Engine connected · {state.mode} mode
              </div>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}