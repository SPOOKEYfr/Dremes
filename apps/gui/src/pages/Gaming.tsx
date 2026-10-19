import { useState } from "react";
import { Card, StatusPill, Button } from "../components/ui";
import { useAppState, engineCommand } from "../state";

export default function Gaming() {
  const { state, resources } = useAppState();
  const [busy, setBusy] = useState(false);
  const waydroidOn = state.waydroid === "Running" || state.waydroid === "Starting";
  const engineOn = state.engine_enabled;

  const runBoost = async (cmd: "GameBoost", on?: boolean) => {
    setBusy(true);
    try {
      await engineCommand(
        cmd,
        on === undefined ? undefined : { play: on ? "boost_on" : "boost_off" }
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>Gaming</h1>
        <p>Play Android games with minimum resource usage.</p>
        {!engineOn && <div className="engine-gate">Engine is OFF — enable it to start Gaming Mode.</div>}
      </header>
      <Card title="Gaming Mode">
        <div className="mode-hero">
          <div>
            <StatusPill
              label={state.mode === "Gaming" ? "Gaming Mode ACTIVE" : "Gaming Mode"}
              status={state.mode === "Gaming" ? "running" : "warning"}
            />
            <p className="hint">
              Waydroid running with capture fully OFF for maximum performance.
            </p>
          </div>
          <Button
            variant="primary"
            disabled={busy || state.mode === "Gaming" || !engineOn}
            onClick={() => { setBusy(true); void engineCommand("GamingMode").finally(() => setBusy(false)); }}
          >
            {busy ? "Starting…" : "START GAMING"}
          </Button>
        </div>
      </Card>
      <div className="grid">
        <Card title="Waydroid">
          <div className="status-row">
            <span>Waydroid</span>
            <StatusPill label={state.waydroid.toUpperCase()} status={waydroidOn ? "running" : "stopped"} />
          </div>
          <div className="status-row">
            <span>Capture</span>
            <StatusPill label="OFF" status="stopped" />
          </div>
          <div className="status-row">
            <span>Streaming</span>
            <StatusPill label={state.streaming.toUpperCase()} status={state.streaming === "Active" ? "running" : "stopped"} />
          </div>
        </Card>
        <Card title="Resource Usage">
          <div className="status-row">
            <span>CPU</span>
            <strong>{resources.cpu_total > 0 ? `${resources.cpu_total.toFixed(0)}%` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>RAM</span>
            <strong>{resources.ram_used_mb > 0 ? `${(resources.ram_used_mb / 1024).toFixed(1)} GiB` : "—"}</strong>
          </div>
          <div className="status-row">
            <span>Waydroid</span>
            <strong>{resources.waydroid_cpu > 0 ? `${resources.waydroid_cpu.toFixed(0)}%` : "—"}</strong>
          </div>
        </Card>
        <Card title="Performance Boosts">
          <div className="boost-row">
            <label className="switch">
              <input type="checkbox" checked={state.game_boost} disabled={busy} onChange={(e) => runBoost("GameBoost", e.target.checked)} />
              <span className="slider" />
            </label>
            <span className="switch-label">Game Boost</span>
          </div>
          {state.game_boost_note && <div className="status-row"><span>Effect</span><strong>{state.game_boost_note}</strong></div>}
          <p className="hint">
            Game Boost prioritizes the running game by keeping waydroid snappy and
            reducing Dremes overhead.
          </p>
        </Card>
      </div>
    </div>
  );
}