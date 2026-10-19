import { useState } from "react";
import { Card, StatusPill } from "../components/ui";
import { useAppState, engineCommand } from "../state";

export default function Wifi() {
  const { state } = useAppState();
  const [busy, setBusy] = useState(false);
  const engineOn = state.engine_enabled;

  const boost = async (on?: boolean) => {
    setBusy(true);
    try {
      await engineCommand(
        "WifiBoost",
        on === undefined ? undefined : { play: on ? "boost_on" : "boost_off" }
      );
    } finally {
      setBusy(false);
    }
  };

  const hasIface = state.wifi_iface.trim().length > 0;

  return (
    <div className="page">
      <header className="page-head">
        <h1>WiFi Booster</h1>
        <p>Disable wireless power-save to cut game latency on your network.</p>
        {!engineOn && <div className="engine-gate">Engine is OFF — enable it to use the WiFi Booster.</div>}
      </header>
      <div className="grid">
        <Card title="WiFi Boost">
          <div className="boost-row">
            <label className="switch">
              <input
                type="checkbox"
                checked={state.wifi_boost}
                disabled={busy || !engineOn || !hasIface}
                onChange={(e) => void boost(e.target.checked)}
              />
              <span className="slider" />
            </label>
            <span className="switch-label">WiFi Boost</span>
          </div>
          {state.wifi_boost_note && (
            <div className="status-row">
              <span>Effect</span>
              <strong>{state.wifi_boost_note}</strong>
            </div>
          )}
          <p className="hint">
            WiFi Boost turns wireless power-save off while enabled, which lowers
            network latency for online games. Toggle it off to restore the
            previous power-save setting.
          </p>
        </Card>
        <Card title="Network Status">
          <div className="status-row">
            <span>Interface</span>
            <strong>{hasIface ? state.wifi_iface : "No wireless interface detected"}</strong>
          </div>
          <div className="status-row">
            <span>Boost</span>
            <StatusPill
              label={state.wifi_boost ? "ACTIVE" : "OFF"}
              status={state.wifi_boost ? "running" : "stopped"}
            />
          </div>
          {hasIface && (
            <div className="status-row">
              <span>Interface Present</span>
              <StatusPill label="ONLINE" status="running" />
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}