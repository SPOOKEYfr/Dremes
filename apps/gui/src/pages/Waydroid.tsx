import { useState } from "react";
import { Card, StatusPill, Button } from "../components/ui";
import { useAppState, engineCommand } from "../state";

export default function Waydroid() {
  const { state } = useAppState();
  const [busy, setBusy] = useState(false);
  const on = state.waydroid === "Running" || state.waydroid === "Starting";
  const engineOn = state.engine_enabled;

  const run = async (cmd: "GamingMode" | "StopWaydroid") => {
    setBusy(true);
    try { await engineCommand(cmd); } finally { setBusy(false); }
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>Waydroid</h1>
        <p>Manage the Android container and session.</p>
        {!engineOn && <div className="engine-gate">Engine is OFF — enable it to control Waydroid.</div>}
      </header>
      <div className="grid">
        <Card title="Waydroid Status">
          <div className="status-row">
            <span>Waydroid</span>
            <StatusPill label={state.waydroid.toUpperCase()} status={on ? "running" : state.waydroid === "Starting" ? "starting" : "stopped"} />
          </div>
          <div className="status-row">
            <span>Container</span>
            <strong className="mono">{state.waydroid_container || "—"}</strong>
          </div>
          <div className="status-row">
            <span>Session</span>
            <strong className="mono">{state.waydroid_session || "—"}</strong>
          </div>
          <div className="status-row">
            <span>ADB Endpoint</span>
            <strong>{state.adb_endpoint || "—"}</strong>
          </div>
          <div className="status-row">
            <span>Vendor</span>
            <strong>MAINLINE</strong>
          </div>
        </Card>
        <Card title="Store / Ops">
          <p className="hint">Starting Waydroid will also enable the session needed for streaming.</p>
          <div className="inline-actions">
            <Button variant="primary" disabled={busy || on || !engineOn} onClick={() => run("GamingMode")}>
              START WAYDROID
            </Button>
            <Button variant="danger" disabled={busy || !on || !engineOn} onClick={() => run("StopWaydroid")}>
              STOP WAYDROID
            </Button>
          </div>
        </Card>
      </div>
    </div>
  );
}