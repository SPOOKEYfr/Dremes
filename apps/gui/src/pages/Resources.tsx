import { Card } from "../components/ui";
import { useAppState } from "../state";

function Meter({ label, value, max = 100, suffix = "%" }: { label: string; value: number; max?: number; suffix?: string }) {
  const pct = Math.min(100, Math.max(0, (value / max) * 100));
  return (
    <div className="meter">
      <div className="meter-head">
        <span>{label}</span>
        <strong>{Math.round(value)}{suffix}</strong>
      </div>
      <div className="meter-track">
        <div className="meter-fill" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function temp(v?: number | null): string {
  return v == null || Number.isNaN(v) ? "—" : `${Math.round(v)}°C`;
}

export default function Resources() {
  const { resources } = useAppState();
  const ramPct = resources.ram_total_mb > 0
    ? (resources.ram_used_mb / resources.ram_total_mb) * 100
    : 0;
  const battery = resources.battery_pct == null || Number.isNaN(resources.battery_pct)
    ? "—"
    : `${Math.round(resources.battery_pct)}%`;
  const fan = resources.fan_rpm == null || Number.isNaN(resources.fan_rpm)
    ? "—"
    : `${Math.round(resources.fan_rpm)} RPM`;

  return (
    <div className="page">
      <header className="page-head">
        <h1>Resources</h1>
        <p>Live CPU, memory, telemetry and power usage.</p>
      </header>
      <Card title="System">
        <Meter label="CPU" value={resources.cpu_total} />
        <Meter label="RAM" value={ramPct} />
        {resources.gpu_util_pct != null && !Number.isNaN(resources.gpu_util_pct) && (
          <Meter label="GPU Occupied" value={resources.gpu_util_pct} />
        )}
      </Card>
      <Card title="Processes">
        <Meter label="Waydroid CPU" value={resources.waydroid_cpu} />
        <Meter label="Capture (scrcpy)" value={resources.capture_cpu} />
        <Meter label="Dremes Engine" value={resources.engine_cpu} />
        <Meter label="ADB" value={resources.adb_cpu} />
      </Card>
      <div className="grid">
        <Card title="Thermals">
          <div className="status-row">
            <span>CPU Temperature</span>
            <strong>{temp(resources.cpu_temp_c)}</strong>
          </div>
          <div className="status-row">
            <span>GPU Temperature</span>
            <strong>{temp(resources.gpu_temp_c)}</strong>
          </div>
          <div className="status-row">
            <span>SSD Temperature</span>
            <strong>{temp(resources.ssd_temp_c)}</strong>
          </div>
        </Card>
        <Card title="Power & Cooling">
          <div className="status-row">
            <span>Battery</span>
            <strong>{battery}</strong>
          </div>
          <div className="status-row">
            <span>Fan</span>
            <strong>{fan}</strong>
          </div>
          <p className="hint">
            Values shown as "—" are not exposed by this machine's drivers and are
            reported truthfully rather than guessed.
          </p>
        </Card>
      </div>
    </div>
  );
}