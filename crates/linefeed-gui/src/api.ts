// Typed invoke/listen wrapper. Browser-safe by construction: without Tauri
// every invoke resolves to null and listeners are no-ops, so `vite preview`
// renders the shell instead of throwing.

import type {
  DevicePayload,
  DumbEvent,
  GuiConfig,
  ModelFetchEvent,
  ScriptPayload,
  StartupInfoPayload,
  StatusPayload,
  TrackerEvent,
} from "./types.ts";

export const EV = {
  tracker: "linefeed://tracker",
  dumb: "linefeed://dumb",
  config: "linefeed://config",
  status: "linefeed://status",
  fetch: "linefeed://model-fetch",
} as const;

interface TauriGlobal {
  core: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };
  event: {
    listen: (
      ev: string,
      cb: (e: { payload: unknown }) => void,
    ) => Promise<() => void>;
  };
}

function tauri(): TauriGlobal | null {
  const w = globalThis as unknown as { __TAURI__?: TauriGlobal };
  return w.__TAURI__ ?? null;
}

export const hasTauri = (): boolean => tauri() !== null;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  const t = tauri();
  if (!t) return null;
  return (await t.core.invoke(cmd, args)) as T;
}

export function on<T>(event: string, cb: (payload: T) => void): void {
  const t = tauri();
  if (!t) return;
  void t.event.listen(event, (e) => cb(e.payload as T));
}

export const api = {
  loadScript: (path: string) => invoke<ScriptPayload>("load_script", { path }),
  availableEngines: () => invoke<string[]>("available_engines"),
  listDevices: () => invoke<DevicePayload[]>("list_devices"),
  start: () => invoke<void>("start"),
  stop: () => invoke<void>("stop"),
  getConfig: () => invoke<GuiConfig>("get_config"),
  getStatus: () => invoke<StatusPayload>("get_status"),
  startupProbe: () => invoke<StartupInfoPayload>("startup_probe"),
  setScrollMode: (mode: string) => invoke<GuiConfig>("set_scroll_mode", { mode }),
  setSpeed: (wpm: number) => invoke<GuiConfig>("set_speed", { wpm }),
  setMirror: (h: boolean, v: boolean) => invoke<GuiConfig>("set_mirror", { h, v }),
  setFont: (px: number) => invoke<GuiConfig>("set_font", { px }),
  setReadingFont: (id: string) => invoke<GuiConfig>("set_reading_font", { id }),
  setReadingZone: (width: number, height: number) =>
    invoke<GuiConfig>("set_reading_zone", { width, height }),
  setLead: (lines: number) => invoke<GuiConfig>("set_lead", { lines }),
  setEngine: (engine: string) => invoke<GuiConfig>("set_engine", { engine }),
  setModel: (model: string) => invoke<GuiConfig>("set_model", { model }),
  setDevice: (device: string) => invoke<GuiConfig>("set_device", { device }),
  setDebugLog: (on: boolean) => invoke<GuiConfig>("set_debug_log", { on }),
  dumbPlay: (playing: boolean) => invoke<GuiConfig>("dumb_play", { playing }),
  dumbSeek: (cursor: number) => invoke<void>("dumb_seek", { cursor }),
  downloadModel: (model?: string) =>
    invoke<void>("download_model", { model: model ?? null }),
  cancelModelFetch: () => invoke<void>("cancel_model_fetch"),
};

export const onTracker = (cb: (e: TrackerEvent) => void) => on(EV.tracker, cb);
export const onDumb = (cb: (e: DumbEvent) => void) => on(EV.dumb, cb);
export const onConfig = (cb: (c: GuiConfig) => void) => on(EV.config, cb);
export const onStatus = (cb: (s: StatusPayload) => void) => on(EV.status, cb);
export const onModelFetch = (cb: (e: ModelFetchEvent) => void) => on(EV.fetch, cb);

export async function toggleFullscreen(): Promise<void> {
  if (!hasTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  await win.setFullscreen(!(await win.isFullscreen()));
}

export async function pickScript(): Promise<string | null> {
  if (!hasTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Scripts", extensions: ["txt", "md"] }],
  });
  return typeof picked === "string" ? picked : null;
}
