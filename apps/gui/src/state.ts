import { useEffect, useState, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  SystemState,
  ResourceSnapshot,
  DiagnosticItem,
  Settings,
  CaptureCatalog,
  CaptureDiagnosis,
  CaptureSessionInfo,
  CaptureRequest,
} from "./types";
import { emptySessionInfo, emptyCaptureSession } from "./types";
import { playCommandSound, playSound, type SoundName } from "./sound";

// Default/empty state so the UI renders before the engine connects.
export const EMPTY_STATE: SystemState = {
  engine: "Off",
  waydroid: "Stopped",
  capture: "Off",
  streaming: "Off",
  mode: "Idle",
  engine_enabled: false,
  adb_endpoint: "",
  adb_connected: false,
  capture_info: { width: 0, height: 0, format: "", fps: 0, dropped_frames: 0, device: "/dev/video10" },
  reconnect_count: 0,
  obs_status: "UNKNOWN",
  stream_boost: false,
  stream_boost_note: "",
  game_boost: false,
  game_boost_note: "",
  wifi_boost: false,
  wifi_boost_note: "",
  wifi_iface: "",
  super_report: "",
  v4l2_detail: "",
  waydroid_container: "",
  waydroid_session: "",
  session: emptySessionInfo(),
  capture_session: emptyCaptureSession(),
};

export const EMPTY_RES: ResourceSnapshot = {
  cpu_total: 0,
  ram_total_mb: 0,
  ram_used_mb: 0,
  engine_cpu: 0,
  engine_ram_mb: 0,
  waydroid_cpu: 0,
  waydroid_ram_mb: 0,
  capture_cpu: 0,
  capture_ram_mb: 0,
  adb_cpu: 0,
  adb_ram_mb: 0,
  cpu_temp_c: null,
  gpu_util_pct: null,
  gpu_temp_c: null,
  ssd_temp_c: null,
  fan_rpm: null,
  battery_pct: null,
};

type CommandName =
  | "EngineOn"
  | "EngineOff"
  | "GamingMode"
  | "StartStreaming"
  | "StopStreaming"
  | "StopWaydroid"
  | "ReconnectToObs"
  | "OpenObs"
  | "CloseObs"
  | "StreamBoost"
  | "GameBoost"
  | "WifiBoost"
  | "Super"
  | "GetState"
  | "GetResources"
  | "Ping"
  | "GetCaptureCatalog"
  | "StartCapture"
  | "StopCapture"
  | "GetCaptureStatus";

/// A global, lightweight notice feed for engine command feedback (errors and
/// confirmations) surfaced on the pages. Avoids wiring prop-drilling just to
/// show whether an intent actually took effect.
type Notice = { id: number; kind: "error" | "success" | "info"; text: string; fading?: boolean };
let notices: Notice[] = [];
let noticeId = 0;
const noticeListeners = new Set<() => void>();

const NOTICE_LIFETIME_MS = 5000;
const FADE_MS = 250;

function fadeOut(id: number) {
  notices = notices.map((n) => (n.id === id ? { ...n, fading: true } : n));
  noticeListeners.forEach((l) => l());
  window.setTimeout(() => {
    if (!notices.some((n) => n.id === id)) return;
    notices = notices.filter((n) => n.id !== id);
    noticeListeners.forEach((l) => l());
  }, FADE_MS);
}

function publishNotice(kind: Notice["kind"], text: string) {
  const entry: Notice = { id: ++noticeId, kind, text };
  notices = [...notices, entry];
  if (notices.length > 8) notices = notices.slice(-8);
  noticeListeners.forEach((l) => l());
  // Auto-dismiss after 5s; the removal itself fades out.
  window.setTimeout(() => fadeOut(entry.id), NOTICE_LIFETIME_MS);
}

export function dismissNotice(id: number) {
  fadeOut(id);
}

/** Remove every notice at once (used by the Notices "Clear all" button). */
export function clearNotices(): void {
  if (notices.length === 0) return;
  notices = notices.map((n) => ({ ...n, fading: true }));
  noticeListeners.forEach((l) => l());
  window.setTimeout(() => {
    notices = [];
    noticeListeners.forEach((l) => l());
  }, FADE_MS);
}

/** Reactive access to recent engine command notices. */
export function useEngineNotices(): Notice[] {
  return useSyncExternalStore(
    (cb) => {
      noticeListeners.add(cb);
      return () => noticeListeners.delete(cb);
    },
    () => notices
  );
}

export interface CommandResult {
  ok: boolean;
  error: string | null;
}

/// Send an intent/query to the Engine daemon via the GUI's Tauri backend.
/// Returns whether the engine accepted the command, and surfaces any error it
/// returned (e.g. "Engine is OFF. Enable it first."). Pass `opts.play` to
/// choose the success sound explicitly (e.g. boost-off), or `null` to silence it.
export async function engineCommand(
  name: CommandName,
  opts?: { play?: SoundName | null; payload?: unknown }
): Promise<CommandResult> {
  let result: CommandResult = { ok: false, error: "Engine did not respond." };
  try {
    const envelope = (opts?.payload !== undefined
      ? await invoke("engine_command_with", { command: name, payload: opts.payload })
      : await invoke("engine_command", { command: name })) as Envelope;
    if (envelope && "Ack" in envelope) {
      result = { ok: envelope.Ack.ok, error: envelope.Ack.error };
    } else if (envelope && "Snapshot" in envelope) {
      result = { ok: true, error: null };
    } else if (envelope && "Catalog" in envelope) {
      result = { ok: true, error: null };
    } else if (envelope && "CaptureStatus" in envelope) {
      result = { ok: true, error: null };
    }
  } catch (err) {
    result = { ok: false, error: String(err) };
  }
  if (!result.ok) {
    publishNotice("error", result.error || "Command failed.");
    playCommandSound(name, false);
  } else {
    publishNotice("success", `${name} accepted.`);
    if (opts?.play != null) {
      playSound(opts.play);
    } else {
      playCommandSound(name, true);
    }
  }
  return result;
}

interface EngineAck {
  ok: boolean;
  error: string | null;
}

interface EngineSnapshot {
  status: SystemState;
  resources: ResourceSnapshot;
  log_tail: LogEvent[];
}

type Envelope =
  | { Ack: EngineAck }
  | { Snapshot: EngineSnapshot }
  | { Catalog: CaptureCatalog }
  | { CaptureStatus: CaptureSessionInfo }
  | { CaptureDiagnosis: CaptureDiagnosis };

/// Fetch the live capture catalog (session, backends, devices) from the Engine.
export async function fetchCatalog(): Promise<CaptureCatalog | null> {
  try {
    const envelope = (await invoke("engine_command", {
      command: "GetCaptureCatalog",
    })) as Envelope;
    return envelope && "Catalog" in envelope ? envelope.Catalog : null;
  } catch {
    return null;
  }
}

/// Fetch the live general-capture session status from the Engine.
export async function fetchCaptureStatus(): Promise<CaptureSessionInfo | null> {
  try {
    const envelope = (await invoke("engine_command", {
      command: "GetCaptureStatus",
    })) as Envelope;
    return envelope && "CaptureStatus" in envelope ? envelope.CaptureStatus : null;
  } catch {
    return null;
  }
}

/// Run the capture-stack health check (safe while the engine is OFF).
export async function fetchDiagnosis(): Promise<CaptureDiagnosis | null> {
  return fetchDiagnosisCommand("CaptureDiagnose");
}

/// Ask the engine to auto-repair what it can without root, then re-diagnose.
export async function fetchFixCapture(): Promise<CaptureDiagnosis | null> {
  return fetchDiagnosisCommand("CaptureFix");
}

async function fetchDiagnosisCommand(command: string): Promise<CaptureDiagnosis | null> {
  try {
    const envelope = (await invoke("engine_command", {
      command,
    })) as Envelope;
    return envelope && "CaptureDiagnosis" in envelope ? envelope.CaptureDiagnosis : null;
  } catch {
    return null;
  }
}

/// Run Dremes' permanent capture repair script (requires root; pkexec shows a
/// polkit password dialog). Returns the script output for display.
export async function runRootFix(): Promise<string> {
  try {
    return (await invoke("run_root_fix")) as string;
  } catch (e) {
    return String(e);
  }
}

/// Check whether the Engine daemon is reachable.
export async function engineHealth(): Promise<boolean> {
  try {
    return (await invoke("engine_health")) as boolean;
  } catch {
    return false;
  }
}

export type View =
  | "dashboard"
  | "gaming"
  | "streaming"
  | "waydroid"
  | "wifi"
  | "resources"
  | "capture"
  | "logs"
  | "settings"
  | "about";

export interface LogEvent {
  timestamp: string;
  level: string;
  component: string;
  message: string;
}

export interface AppState {
  state: SystemState;
  resources: ResourceSnapshot;
  diagnostics: DiagnosticItem[];
  logs: LogEvent[];
  version: string;
  connected: boolean;
}

function parseSnapshot(body?: EngineSnapshot): Partial<AppState> {
  if (!body) return {};
  return {
    state: body.status,
    resources: body.resources,
    logs: body.log_tail ?? [],
  };
}

export function useAppState(): AppState {
  const [state, setState] = useState<SystemState>(EMPTY_STATE);
  const [resources, setResources] = useState<ResourceSnapshot>(EMPTY_RES);
  const [diagnostics, setDiagnostics] = useState<DiagnosticItem[]>([]);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [version, setVersion] = useState("Dremes");
  const [connected, setConnected] = useState(false);

  const refresh = () => {
    try {
      void (async () => {
        const healthy = await engineHealth();
        setConnected(healthy);
        if (!healthy) return;
        const envelope = (await invoke("engine_command", { command: "GetState" })) as Envelope;
        const snap = envelope && "Snapshot" in envelope ? envelope.Snapshot : undefined;
        const parsed = parseSnapshot(snap);
        if (parsed.state) setState(parsed.state);
        if (parsed.resources) setResources(parsed.resources);
        if (parsed.logs) setLogs(parsed.logs);
        void (async () => {
          try {
            const v = (await invoke("dremes_version")) as string;
            if (v) setVersion(v);
          } catch { /* ignore */ }
        })();
      })();
    } catch {
      setConnected(false);
    }
  };

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 3000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { state, resources, diagnostics, logs, version, connected };
}

export function engineLabel(enabled: boolean): string {
  return enabled ? "ENGINE RUNNING" : "ENGINE OFF";
}

/// Apply the user's theme choice to the app root. "system" removes the
/// attribute so the OS media query takes over.
export function applyTheme(theme: string | undefined): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.setAttribute("data-theme", theme);
  } else {
    root.removeAttribute("data-theme");
  }
}