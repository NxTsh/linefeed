// Hand-mirrored serde payloads from src-tauri (contract-tested in
// tests/contract.test.ts against the Rust source).

export interface WordPayload {
  raw: string;
  /** Token span [ts, te) in tracker index space. */
  ts: number;
  te: number;
  line: number;
  para: number;
}

export interface ParagraphPayload {
  start_line: number;
  end_line: number;
}

export interface ScriptPayload {
  path: string;
  words: WordPayload[];
  paragraphs: ParagraphPayload[];
  n_tokens: number;
  n_words: number;
}

export type TrackState = "TRACKING" | "HOLDING" | "LOST" | "BACKTRACK";

export interface TrackerEvent {
  t: number;
  state: TrackState;
  cursor: number;
  score: number;
  jump: number;
  held_for: number;
}

export interface DumbEvent {
  t: number;
  cursor: number;
  wpm: number;
  playing: boolean;
}

export interface GuiConfig {
  engine: string;
  device: string;
  scroll_mode: "voice" | "dumb";
  wpm: number;
  font_px: number;
  reading_font: string;
  mirror_h: boolean;
  mirror_v: boolean;
  reading_width: number;
  reading_height: number;
  lead_lines: number;
  debug_log: boolean;
  last_script: string;
}

export interface StatusPayload {
  running: boolean;
  engine: string;
  device: string;
  state: "idle" | "loading-model" | "listening" | "stopped" | "error";
  message: string;
}

export interface DevicePayload {
  index: number;
  name: string;
  default: boolean;
  configs: string;
}

export interface StartupInfoPayload {
  engines: string[];
  engine: string;
  models_dir: string;
  models_ok: boolean;
  missing: string[];
  fetchable: boolean;
  fetch_url: string;
  fetch_bytes: number;
  last_script: string;
}

export interface ModelFetchEvent {
  phase:
    | "starting"
    | "downloading"
    | "retrying"
    | "extracting"
    | "ready"
    | "cancelled"
    | "fatal";
  downloaded: number;
  total: number;
  pct: number;
  message: string;
  fatal: boolean;
  curl: string;
}
