import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card, Button } from "../components/ui";
import { applyTheme } from "../state";
import { setSoundVolume, previewTick, playSound } from "../sound";
import type { Settings } from "../types";

function Toggle({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="setting-item">
      <div>
        <strong>{label}</strong>
        {desc && <small>{desc}</small>}
      </div>
      <span className={`switch small ${checked ? "on" : ""}`}>
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => {
            onChange(e.target.checked);
            playSound("toggle");
          }}
        />
        <span className="slider" />
      </span>
    </label>
  );
}

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then((s) => {
        setSettings(s);
        applyTheme(s.theme);
        setSoundVolume(s.sound_volume ?? 0.7);
      })
      .catch(() => {
      setSettings({
        start_engine_automatically: false,
        start_gui_automatically: false,
        minimize_to_tray: true,
        start_minimized: false,
        theme: "system",
        notifications: true,
        sound_volume: 0.7,
        waydroid_path: "waydroid",
        waydroid_session_recovery: true,
        waydroid_retry_limit: 3,
        capture: { device: "/dev/video10", auto_detect: true, fps_target: 30, retry_delay_secs: 2, max_retries: 5 },
        adb: { auto_discovery: true, retry_behavior: true, timeout_secs: 10 },
      });
    });
  }, []);

  if (!settings) return <div className="page">Loading…</div>;

  const set = (patch: Partial<Settings>) => setSettings((s) => ({ ...s!, ...patch }));
  const setCap = (patch: Partial<Settings["capture"]>) =>
    setSettings((s) => ({ ...s!, capture: { ...s!.capture, ...patch } }));
  const setAdb = (patch: Partial<Settings["adb"]>) =>
    setSettings((s) => ({ ...s!, adb: { ...s!.adb, ...patch } }));

  const save = () => {
    playSound("save");
    invoke("save_settings", { settings }).then(() => alert("Settings saved."));
  };

  return (
    <div className="page">
      <header className="page-head">
        <h1>Settings</h1>
        <p>Configure Dremes behavior.</p>
      </header>

      <Card title="General">
        <Toggle label="Start Dremes Engine automatically" desc="Default OFF" checked={settings.start_engine_automatically} onChange={(v) => set({ start_engine_automatically: v })} />
        <Toggle label="Start GUI automatically" desc="Default OFF" checked={settings.start_gui_automatically} onChange={(v) => set({ start_gui_automatically: v })} />
        <Toggle label="Minimize to tray" checked={settings.minimize_to_tray} onChange={(v) => set({ minimize_to_tray: v })} />
        <Toggle label="Start minimized" checked={settings.start_minimized} onChange={(v) => set({ start_minimized: v })} />
        <Toggle label="Notifications" checked={settings.notifications} onChange={(v) => set({ notifications: v })} />
        <div className="setting-item">
          <div><strong>Theme</strong></div>
          <select value={settings.theme} onChange={(e) => { set({ theme: e.target.value }); applyTheme(e.target.value); playSound("toggle"); }}>
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </div>
      </Card>

      <Card title="Audio">
        <div className="setting-item">
          <div>
            <strong>Sound volume</strong>
            <small>Button and engine sounds. Drag to preview — saved with "Save Settings".</small>
          </div>
          <input
            type="range"
            min={0}
            max={100}
            value={Math.round((settings.sound_volume ?? 0.7) * 100)}
            onChange={(e) => {
              const v = Number(e.target.value) / 100;
              set({ sound_volume: v });
              setSoundVolume(v);
              if (v > 0) previewTick();
            }}
          />
        </div>
        <div className="setting-item">
          <div>
            <strong>Test sound</strong>
            <small>Preview the engine and SUPER signature sounds.</small>
          </div>
          <div className="inline-actions">
            <Button variant="ghost" onClick={() => { playSound("jet_start"); }}>Engine Start</Button>
            <Button variant="ghost" onClick={() => { playSound("jet_stop"); }}>Engine Stop</Button>
            <Button variant="ghost" onClick={() => { playSound("super"); }}>SUPER</Button>
          </div>
        </div>
      </Card>

      <Card title="Waydroid">
        <Toggle label="Auto-recover session" checked={settings.waydroid_session_recovery} onChange={(v) => set({ waydroid_session_recovery: v })} />
        <div className="setting-item">
          <div><strong>Retry limit</strong><small>Automatic recovery retries before giving up.</small></div>
          <input type="number" value={settings.waydroid_retry_limit} onChange={(e) => set({ waydroid_retry_limit: Number(e.target.value) })} />
        </div>
      </Card>

      <Card title="Capture">
        <div className="setting-item">
          <div><strong>V4L2 device</strong></div>
          <input value={settings.capture.device} onChange={(e) => setCap({ device: e.target.value })} />
        </div>
        <Toggle label="Auto-detect device" checked={settings.capture.auto_detect} onChange={(v) => setCap({ auto_detect: v })} />
        <div className="setting-item">
          <div><strong>FPS target</strong></div>
          <input type="number" value={settings.capture.fps_target} onChange={(e) => setCap({ fps_target: Number(e.target.value) })} />
        </div>
        <div className="setting-item">
          <div><strong>Max retries</strong></div>
          <input type="number" value={settings.capture.max_retries} onChange={(e) => setCap({ max_retries: Number(e.target.value) })} />
        </div>
      </Card>

      <Card title="ADB">
        <Toggle label="Auto-discovery of endpoint" checked={settings.adb.auto_discovery} onChange={(v) => setAdb({ auto_discovery: v })} />
        <Toggle label="Retry behavior" checked={settings.adb.retry_behavior} onChange={(v) => setAdb({ retry_behavior: v })} />
        <div className="setting-item">
          <div><strong>Timeout (s)</strong></div>
          <input type="number" value={settings.adb.timeout_secs} onChange={(e) => setAdb({ timeout_secs: Number(e.target.value) })} />
        </div>
      </Card>

      <Card title="Advanced">
        <Button variant="danger" onClick={() => { if (confirm("Reset Dremes configuration to defaults?")) {
          setSettings({
          start_engine_automatically: false, start_gui_automatically: false, minimize_to_tray: true,
          start_minimized: false, theme: "system", notifications: true, sound_volume: 0.7, waydroid_path: "waydroid",
          waydroid_session_recovery: true, waydroid_retry_limit: 3,
          capture: { device: "/dev/video10", auto_detect: true, fps_target: 30, retry_delay_secs: 2, max_retries: 5 },
          adb: { auto_discovery: true, retry_behavior: true, timeout_secs: 10 },
          });
          playSound("reset");
        } }}>
          Reset Dremes Configuration
        </Button>
      </Card>

      <div className="save-bar">
        <Button variant="primary" onClick={save}>Save Settings</Button>
      </div>
    </div>
  );
}
