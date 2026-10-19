// Shared frontend types mirroring the Rust backend state.

export type EngineState =
  | "Off"
  | "Starting"
  | "Running"
  | "Stopping"
  | "Error";

export type WaydroidState =
  | "Stopped"
  | "Starting"
  | "Running"
  | "Stopping"
  | "Error"
  | "Unknown";

export type CaptureState =
  | "Off"
  | "Starting"
  | "Running"
  | "Reconnecting"
  | "Stopping"
  | "Error";

export type StreamingState =
  | "Off"
  | "Starting"
  | "Active"
  | "Stopping"
  | "Error";

export type Mode = "Idle" | "Gaming" | "Streaming";

export type CaptureRunState =
  | "Idle"
  | "Starting"
  | "Active"
  | "Stopping"
  | "Error";

export type CaptureSourceKind =
  | "Mobile"
  | "Game"
  | "Window"
  | "Monitor"
  | "Camera"
  | "V4l2Device";

export interface SessionInfo {
  session_type: string;
  desktop: string;
  desktop_family: string;
  wayland: boolean;
  display: string;
  wayland_display: string;
}

export interface CaptureSessionInfo {
  state: CaptureRunState;
  kind: CaptureSourceKind | null;
  source: string;
  output_device: string;
  method: string;
  connection: string;
  width: number;
  height: number;
  fps: number;
  started_at_unix: number;
  error: string;
}

export interface CaptureRequest {
  kind: CaptureSourceKind;
  source: string;
  output_device: string;
  width: number;
  height: number;
  fps: number;
}

export interface V4l2DeviceInfo {
  path: string;
  name: string;
  is_loopback: boolean;
  is_capture: boolean;
  is_output: boolean;
  is_reserved: boolean;
}

export interface MobileDeviceInfo {
  serial: string;
  state: string;
  product: string;
  model: string;
  transport: string;
  online: boolean;
}

export interface CaptureBackendStatus {
  available: boolean;
  notes: string[];
}

export interface CaptureCatalog {
  session: SessionInfo;
  backend_status: CaptureBackendStatus;
  output_devices: V4l2DeviceInfo[];
  source_devices: V4l2DeviceInfo[];
  mobile_devices: MobileDeviceInfo[];
}

export function emptySessionInfo(): SessionInfo {
  return {
    session_type: "unknown",
    desktop: "unknown",
    desktop_family: "unknown",
    wayland: false,
    display: "",
    wayland_display: "",
  };
}

export interface CaptureDiagnosis {
  healthy: boolean;
  issues: string[];
  notes: string[];
  root_actions: string[];
}

export function emptyCaptureSession(): CaptureSessionInfo {
  return {
    state: "Idle",
    kind: null,
    source: "",
    output_device: "",
    method: "",
    connection: "",
    width: 0,
    height: 0,
    fps: 0,
    started_at_unix: 0,
    error: "",
  };
}

export interface CaptureInfo {
  width: number;
  height: number;
  format: string;
  fps: number;
  dropped_frames: number;
  device: string;
}

export interface SystemState {
  engine: EngineState;
  waydroid: WaydroidState;
  capture: CaptureState;
  streaming: StreamingState;
  mode: Mode;
  engine_enabled: boolean;
  adb_endpoint: string;
  adb_connected: boolean;
  capture_info: CaptureInfo;
  reconnect_count: number;
  obs_status: string;
  stream_boost: boolean;
  stream_boost_note: string;
  game_boost: boolean;
  game_boost_note: string;
  wifi_boost: boolean;
  wifi_boost_note: string;
  wifi_iface: string;
  super_report: string;
  v4l2_detail: string;
  waydroid_container: string;
  waydroid_session: string;
  session: SessionInfo;
  capture_session: CaptureSessionInfo;
}

export interface ResourceSnapshot {
  cpu_total: number;
  ram_total_mb: number;
  ram_used_mb: number;
  engine_cpu: number;
  engine_ram_mb: number;
  waydroid_cpu: number;
  waydroid_ram_mb: number;
  capture_cpu: number;
  capture_ram_mb: number;
  adb_cpu: number;
  adb_ram_mb: number;
  cpu_temp_c?: number | null;
  gpu_util_pct?: number | null;
  gpu_temp_c?: number | null;
  ssd_temp_c?: number | null;
  fan_rpm?: number | null;
  battery_pct?: number | null;
}

export interface DiagnosticItem {
  key: string;
  label: string;
  status: "pass" | "warning" | "fail";
  message: string;
}

export interface Settings {
  start_engine_automatically: boolean;
  start_gui_automatically: boolean;
  minimize_to_tray: boolean;
  start_minimized: boolean;
  theme: string;
  notifications: boolean;
  sound_volume: number;
  waydroid_path: string;
  waydroid_session_recovery: boolean;
  waydroid_retry_limit: number;
  capture: {
    device: string;
    auto_detect: boolean;
    fps_target: number;
    retry_delay_secs: number;
    max_retries: number;
  };
  adb: {
    auto_discovery: boolean;
    retry_behavior: boolean;
    timeout_secs: number;
  };
}
