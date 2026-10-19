import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, StatusPill } from "../components/ui";
import {
  useAppState,
  engineCommand,
  fetchCatalog,
  fetchCaptureStatus,
  fetchDiagnosis,
  fetchFixCapture,
  runRootFix,
} from "../state";
import type {
  CaptureBackendStatus,
  CaptureCatalog,
  CaptureDiagnosis,
  CaptureRequest,
  CaptureSessionInfo,
  CaptureSourceKind,
} from "../types";

const KIND_LABEL: Record<CaptureSourceKind, string> = {
  Mobile: "Mobile (ADB)",
  Game: "Game",
  Window: "Window",
  Monitor: "Monitor",
  Camera: "Camera",
  V4l2Device: "V4L2 Device",
};

const KIND_ORDER: CaptureSourceKind[] = [
  "Mobile",
  "Game",
  "Window",
  "Monitor",
  "Camera",
  "V4l2Device",
];

function kindNote(
  kind: CaptureSourceKind,
  catalog: CaptureCatalog | null,
  wayland: boolean
): { ok: boolean; text: string } {
  if (!catalog) return { ok: true, text: "Loading backend status…" };
  const notes = catalog.backend_status.notes.join(" ");
  const installed = (tool: string) =>
    notes.includes(`${tool}: installed`);
  switch (kind) {
    case "Mobile":
      if (!installed("scrcpy") || !installed("adb"))
        return { ok: false, text: "Mobile capture needs scrcpy and adb (both checked above)." };
      if (catalog.mobile_devices.length === 0)
        return { ok: false, text: "No ADB device is connected. Plug in a phone (USB debugging on) or an emulator." };
      return { ok: true, text: `${catalog.mobile_devices.filter((d) => d.online).length} online ADB device(s).` };
    case "Game":
      if (!installed("OBS-VKCapture layer"))
        return { ok: false, text: "obs-vkcapture is not installed — native game capture is unavailable. Enable Game Capture in OBS for Vulkan/OpenGL games instead." };
      return { ok: true, text: "Game capture streams the active application via OBS Game Capture." };
    case "Window":
      if (wayland)
        return { ok: false, text: "Native window capture under Wayland is provided by OBS Studio's Window Capture, not Dremes. On X11, xdotool + ffmpeg can capture a single window." };
      if (!installed("ffmpeg"))
        return { ok: false, text: "Window capture needs ffmpeg (missing)." };
      return { ok: true, text: `Captures the window matching the title via ffmpeg (${installed("xdotool") ? "xdotool" : "click-to-select"}) on ${catalog.session.display || "the current display"}.` };
    case "Monitor":
      if (wayland)
        return {
          ok: false,
          text: "Monitor capture on Wayland needs wf-recorder, which works only on wlroots compositors (e.g. LabWC). Your compositor is GNOME, so use OBS Studio > Screen Capture instead.",
        };
      if (!installed("ffmpeg"))
        return { ok: false, text: "X11 monitor capture needs ffmpeg (missing)." };
      return { ok: true, text: "Whole-screen capture via ffmpeg x11grab → V4L2." };
    case "Camera":
    case "V4l2Device":
      if (!installed("ffmpeg"))
        return { ok: false, text: `${KIND_LABEL[kind]} capture needs ffmpeg (missing).` };
      return { ok: true, text: "Captures a V4L2 input device via ffmpeg and forwards it to a loopback output." };
  }
}

function statePill(state: CaptureSessionInfo["state"]) {
  switch (state) {
    case "Active":
      return <StatusPill label="RUNNING" status="running" />;
    case "Starting":
      return <StatusPill label="STARTING" status="starting" />;
    case "Stopping":
      return <StatusPill label="STOPPING" status="starting" />;
    case "Error":
      return <StatusPill label="ERROR" status="bad" />;
    default:
      return <StatusPill label="IDLE" status="stopped" />;
  }
}

export default function Capture() {
  const { state } = useAppState();
  const [catalog, setCatalog] = useState<CaptureCatalog | null>(null);
  const [status, setStatus] = useState<CaptureSessionInfo | null>(null);
  const [diagnosis, setDiagnosis] = useState<CaptureDiagnosis | null>(null);
  const [rootFixOut, setRootFixOut] = useState("");
  const [fixBusy, setFixBusy] = useState(false);
  const [kind, setKind] = useState<CaptureSourceKind>("Monitor");
  const [source, setSource] = useState("");
  const [output, setOutput] = useState("auto");
  const [width, setWidth] = useState("");
  const [height, setHeight] = useState("");
  const [fps, setFps] = useState("");
  const [busy, setBusy] = useState(false);

  const engineOn = state.engine_enabled;
  const session = useMemo(() => catalog?.session ?? state.session, [catalog, state.session]);
  const active = status?.state === "Active" || status?.state === "Starting" || status?.state === "Stopping";
  const isActive = (status?.state ?? "Idle") === "Active";

  const refresh = () => {
    void (async () => {
      const cat = await fetchCatalog();
      if (cat) setCatalog(cat);
      const st = await fetchCaptureStatus();
      if (st) setStatus(st);
      const diag = await fetchDiagnosis();
      if (diag) setDiagnosis(diag);
    })();
  };

  // Fresh catalog + status while this page is open.
  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cameras = useMemo(
    () =>
      catalog?.source_devices.filter(
        (d) => d.is_capture && !d.is_loopback
      ) ?? [],
    [catalog]
  );
  const v4l2Sources = useMemo(
    () => catalog?.source_devices.filter((d) => d.is_capture && !d.is_loopback) ?? [],
    [catalog]
  );
  const outputs = useMemo(
    () => catalog?.output_devices.filter((d) => d.is_output) ?? [],
    [catalog]
  );
  const mobiles = useMemo(
    () => catalog?.mobile_devices.filter((d) => d.online) ?? [],
    [catalog]
  );

  const note = kindNote(kind, catalog, session.wayland);

  // True once the user picks a source kind themselves; auto-selection stops.
  const manualKind = useRef(false);

  const pickKind = (k: CaptureSourceKind, opts?: { silent?: boolean }) => {
    setKind(k);
    setSource("");
    switch (k) {
      case "Mobile":
        if (mobiles[0]) setSource(mobiles[0].serial);
        break;
      case "Camera":
        if (cameras[0]) setSource(cameras[0].path);
        break;
      case "V4l2Device":
        if (v4l2Sources[0]) setSource(v4l2Sources[0].path);
        break;
      default:
        break;
    }
    if (!opts?.silent) manualKind.current = true;
  };

  // Kinds that could genuinely start right now, in preference order.
  const startableKinds = useMemo(
    () =>
      KIND_ORDER.filter((k) => {
        if (k === "Camera") return cameras.length > 0;
        if (k === "V4l2Device") return v4l2Sources.length > 0;
        if (k === "Mobile") return mobiles.length > 0;
        return kindNote(k, catalog, session.wayland).ok;
      }),
    [catalog, cameras, v4l2Sources, mobiles, session.wayland]
  );

  // Until the user picks a kind themselves, open the tab on one that can
  // actually start (Camera on most hosts) instead of Monitor.
  useEffect(() => {
    if (!catalog || manualKind.current) return;
    if (startableKinds.length > 0 && !startableKinds.includes(kind)) {
      pickKind(startableKinds[0], { silent: true });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalog, startableKinds]);

  const resolveSource = (): string => {
    switch (kind) {
      case "Mobile":
        return source || mobiles[0]?.serial || "";
      case "Camera":
        return source || cameras[0]?.path || "";
      case "V4l2Device":
        return source || v4l2Sources[0]?.path || "";
      case "Window":
        return source.trim();
      default:
        return "";
    }
  };

  const start = async () => {
    const req: CaptureRequest = {
      kind,
      source: resolveSource(),
      output_device: output === "auto" ? "" : output,
      width: width ? Number(width) : 0,
      height: height ? Number(height) : 0,
      fps: fps ? Number(fps) : 0,
    };
    setBusy(true);
    try {
      await engineCommand("StartCapture", { payload: req, play: null });
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const stop = async () => {
    setBusy(true);
    try {
      await engineCommand("StopCapture", { play: "boost_off" });
    } finally {
      setBusy(false);
      refresh();
    }
  };

  // Fix All: let the engine auto-repair what it can without root, then re-check.
  const runFixAll = async () => {
    setFixBusy(true);
    try {
      const d = await fetchFixCapture();
      if (d) setDiagnosis(d);
    } finally {
      setFixBusy(false);
    }
  };

  // Apply permanent repairs (root via pkexec): loopback persistence across reboots.
  const runRoot = async () => {
    setFixBusy(true);
    setRootFixOut("");
    try {
      const out = await runRootFix();
      setRootFixOut(out);
    } finally {
      setFixBusy(false);
    }
  };

  function resolveSourcePresent(): boolean {
    if (kind === "Camera") return cameras.length > 0;
    if (kind === "V4l2Device") return v4l2Sources.length > 0;
    if (kind === "Mobile") return mobiles.length > 0;
    return true;
  }
  const canStart =
    engineOn && !active && note.ok && resolveSourcePresent();

  const backendBadge = (bs: CaptureBackendStatus) =>
    bs.available ? (
      <StatusPill label="BACKENDS READY" status="running" />
    ) : (
      <StatusPill label="NO CAPTURE BACKEND" status="bad" />
    );

  return (
    <div className="page">
      <header className="page-head">
        <h1>Capture</h1>
        <p>
          Capture any source to a loopback V4L2 device for OBS — independent of
          the Waydroid pipeline.
        </p>
        {!engineOn && (
          <div className="engine-gate">
            Engine is OFF — enable it to start captures.
          </div>
        )}
      </header>

      <div className="grid">
        <Card title="Session">
          <div className="status-row">
            <span>Session Type</span>
            <strong>{session.session_type}</strong>
          </div>
          <div className="status-row">
            <span>Desktop</span>
            <strong>{session.desktop}</strong>
          </div>
          <div className="status-row">
            <span>Display</span>
            <strong>
              {session.wayland
                ? session.wayland_display || "Wayland"
                : session.display || "—"}
            </strong>
          </div>
        </Card>

        <Card title="Capture Backends">
          {catalog ? (
            <>
              <div className="status-row">
                <span>Status</span>
                {backendBadge(catalog.backend_status)}
              </div>
              {catalog.backend_status.notes.map((n, i) => (
                <div key={i} className="status-row tool-note">
                  <span className="tool-note-dot">▪</span>
                  <strong>{n}</strong>
                </div>
              ))}
            </>
          ) : (
            <p className="hint">Loading backend status…</p>
          )}
        </Card>

        <Card title="Capture Health">
          {diagnosis ? (
            <>
              <div className="status-row">
                <span>Status</span>
                {diagnosis.healthy ? (
                  <StatusPill label="ALL GOOD" status="running" />
                ) : (
                  <StatusPill label="NEEDS ATTENTION" status="bad" />
                )}
              </div>
              {diagnosis.notes.map((n, i) => (
                <div key={`n${i}`} className="status-row tool-note">
                  <span className="tool-note-dot">◉</span>
                  <strong>{n}</strong>
                </div>
              ))}
              {diagnosis.issues.map((n, i) => (
                <div key={`i${i}`} className="status-row tool-note cap-note warn">
                  <span className="tool-note-dot">⚠</span>
                  <strong>{n}</strong>
                </div>
              ))}
              <div className="actions">
                <Button
                  variant="primary"
                  disabled={fixBusy}
                  onClick={() => void runFixAll()}
                >
                  {fixBusy ? "Fixing…" : "Fix All"}
                </Button>
                {diagnosis.root_actions.length > 0 && (
                  <Button disabled={fixBusy} onClick={() => void runRoot()}>
                    Apply permanent fixes (password)
                  </Button>
                )}
              </div>
              {diagnosis.root_actions.length > 0 && (
                <p className="hint">
                  "Apply permanent fixes" runs one root command (polkit password
                  dialog) that makes the loopback capture outputs survive reboots.
                </p>
              )}
              {rootFixOut && <pre className="fix-out">{rootFixOut}</pre>}
            </>
          ) : (
            <p className="hint">Checking capture health…</p>
          )}
        </Card>
      </div>

      <div className="grid">
        <Card title="Start a Capture">
          <div className="kind-grid">
            {KIND_ORDER.map((k) => (
              <button
                key={k}
                className={`kind-btn ${kind === k ? "active" : ""}`}
                onClick={() => pickKind(k)}
                disabled={busy || active}
              >
                {KIND_LABEL[k]}
              </button>
            ))}
          </div>

          {kind === "Mobile" && (
            <label className="field">
              <span>ADB Device</span>
              <select
                value={source}
                onChange={(e) => setSource(e.target.value)}
                disabled={busy || active}
              >
                {mobiles.map((d) => (
                  <option key={d.serial} value={d.serial}>
                    {d.model || "Android"} — {d.serial} ({d.state})
                  </option>
                ))}
              </select>
            </label>
          )}

          {kind === "Camera" && (
            <label className="field">
              <span>Camera Device</span>
              <select
                value={source}
                onChange={(e) => setSource(e.target.value)}
                disabled={busy || active}
              >
                {cameras.map((d) => (
                  <option key={d.path} value={d.path}>
                    {d.name} — {d.path}
                  </option>
                ))}
              </select>
            </label>
          )}

          {kind === "V4l2Device" && (
            <label className="field">
              <span>Source Device</span>
              <select
                value={source}
                onChange={(e) => setSource(e.target.value)}
                disabled={busy || active}
              >
                {v4l2Sources.map((d) => (
                  <option key={d.path} value={d.path}>
                    {d.name} — {d.path}
                  </option>
                ))}
              </select>
            </label>
          )}

          {kind === "Window" && (
            <label className="field">
              <span>Window Title</span>
              <input
                type="text"
                value={source}
                placeholder="e.g. Firefox, or leave empty to pick with xdotool"
                onChange={(e) => setSource(e.target.value)}
                disabled={busy || active}
              />
            </label>
          )}

          {outputs.length > 0 ? (
            <label className="field">
              <span>Output Device</span>
              <select
                value={output}
                onChange={(e) => setOutput(e.target.value)}
                disabled={busy || active}
              >
                <option value="auto">Auto (first free loopback)</option>
                {outputs.map((d) => (
                  <option key={d.path} value={d.path}>
                    {d.name} — {d.path}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <p className="hint">
              No free loopback output device. Load v4l2loopback with an extra
              node (e.g. <code>video_nr=10,11</code>) so captures have somewhere
              to write.
            </p>
          )}

          {(kind === "Monitor" || kind === "Camera" || kind === "V4l2Device") && (
            <div className="grid compact">
              <label className="field">
                <span>Width</span>
                <input
                  type="number"
                  value={width}
                  placeholder="auto"
                  onChange={(e) => setWidth(e.target.value)}
                  disabled={busy || active}
                />
              </label>
              <label className="field">
                <span>Height</span>
                <input
                  type="number"
                  value={height}
                  placeholder="auto"
                  onChange={(e) => setHeight(e.target.value)}
                  disabled={busy || active}
                />
              </label>
              <label className="field">
                <span>FPS</span>
                <input
                  type="number"
                  value={fps}
                  placeholder="auto"
                  onChange={(e) => setFps(e.target.value)}
                  disabled={busy || active}
                />
              </label>
            </div>
          )}

          <div className="actions">
            <Button variant="primary" disabled={!canStart || busy} onClick={() => void start()}>
              Start Capture
            </Button>
            <Button
              variant="danger"
              disabled={!active || busy}
              onClick={() => void stop()}
            >
              Stop Capture
            </Button>
          </div>

          {!engineOn && (
            <p className="cap-note warn">
              Engine is OFF — flip it on from the Dashboard, then Start will
              work.
            </p>
          )}

          <p className={`cap-note ${note.ok ? "" : "warn"}`}>{note.text}</p>

          {!note.ok && startableKinds.length > 0 && (
            <div className="cap-suggest">
              <span>You can start right now:</span>
              {startableKinds.map((k) => (
                <button
                  key={k}
                  className="kind-btn mini"
                  onClick={() => pickKind(k)}
                >
                  {KIND_LABEL[k]}
                </button>
              ))}
            </div>
          )}

          {status?.error && status.state === "Error" && (
            <div className="cap-error">
              <strong>Last error:</strong> {status.error}
            </div>
          )}
        </Card>

        <Card title="Active Capture">
          {status ? (
            <>
              <div className="status-row">
                <span>State</span>
                {statePill(status.state)}
              </div>
              {isActive && (
                <>
                  <div className="status-row">
                    <span>Source</span>
                    <strong>{status.kind ? KIND_LABEL[status.kind] : "—"} {status.source ? `· ${status.source}` : ""}</strong>
                  </div>
                  <div className="status-row">
                    <span>Method</span>
                    <strong>{status.method || "—"}</strong>
                  </div>
                  <div className="status-row">
                    <span>Output</span>
                    <strong>{status.output_device || "—"}</strong>
                  </div>
                  {status.connection && (
                    <div className="status-row">
                      <span>Connection</span>
                      <strong>{status.connection}</strong>
                    </div>
                  )}
                  <div className="status-row">
                    <span>Resolution</span>
                    <strong>
                      {status.width && status.height
                        ? `${status.width}×${status.height}`
                        : "auto"}
                      {status.fps ? ` @ ${status.fps} FPS` : ""}
                    </strong>
                  </div>
                </>
              )}
              {!isActive && status.error && (
                <div className="cap-error">
                  <strong>Last error:</strong> {status.error}
                </div>
              )}
            </>
          ) : (
            <p className="hint">Nothing running. Pick a source and press Start.</p>
          )}
        </Card>
      </div>
    </div>
  );
}