// The overlay controller. Every action goes through api.*; state comes back
// via config/status events. This module owns the SINGLE visibility funnel:
// applyVisibility() is the only place that touches chrome visibility
// classes (contract-tested by grep).

import { api, pickScript, toggleFullscreen } from "./api.ts";
import { CONTROLS_TEMPLATE } from "./controls-template.ts";
import { fmtMB } from "./model-fetch.ts";
import { AUTO_HIDE_MS } from "./present.ts";
import { READING_FONTS, MAX_LEAD_LINES } from "./pipeline.ts";
import { zoneStep, ZONE_HEIGHT_RANGE, ZONE_STEP_PCT, ZONE_WIDTH_RANGE } from "./zone.ts";
import type {
  GuiConfig,
  ModelFetchEvent,
  ModelInfoPayload,
  StartupInfoPayload,
  StatusPayload,
  TrackState,
} from "./types.ts";

export class Controls {
  private cfg: GuiConfig | null = null;
  private root: HTMLElement;
  private lastActivity = 0;
  private chromeHidden = false;
  private playing = false;
  private models: ModelInfoPayload[] = [];
  private activeFetch: ModelFetchEvent | null = null;

  onOpen: (path: string) => void = () => {};
  onPresent: () => void = () => {};
  onResume: () => void = () => {};

  constructor(rootEl: HTMLElement) {
    this.root = rootEl;
    this.root.innerHTML = CONTROLS_TEMPLATE;

    const fontSel = this.el<HTMLSelectElement>("reading-font");
    for (const f of READING_FONTS) {
      const opt = document.createElement("option");
      opt.value = f.id;
      opt.textContent = f.label;
      fontSel.appendChild(opt);
    }

    this.wire();
    this.touch();
    document.addEventListener("pointermove", () => this.touch());
    document.addEventListener("keydown", () => this.touch());
    window.setInterval(() => this.applyVisibility(), 500);
  }

  private el<T extends HTMLElement = HTMLElement>(id: string): T {
    return this.root.querySelector(`[data-c="${id}"]`) as T;
  }

  private panel(): HTMLElement {
    return document.getElementById("settings-panel")!;
  }

  private wire(): void {
    this.el("open").addEventListener("click", async () => {
      const path = await pickScript();
      if (path) this.onOpen(path);
    });
    this.el("mode").addEventListener("click", () => {
      const next = this.cfg?.scroll_mode === "dumb" ? "voice" : "dumb";
      void api.setScrollMode(next);
    });
    this.el("start").addEventListener("click", () => {
      void api.start();
    });
    this.el<HTMLSelectElement>("device").addEventListener("change", (e) => {
      void api.setDevice((e.target as HTMLSelectElement).value);
    });
    this.el("play").addEventListener("click", () => {
      void api.dumbPlay(!this.playing);
    });
    this.el("wpm-down").addEventListener("click", () =>
      api.setSpeed((this.cfg?.wpm ?? 140) - 10),
    );
    this.el("wpm-up").addEventListener("click", () =>
      api.setSpeed((this.cfg?.wpm ?? 140) + 10),
    );
    this.el("font-down").addEventListener("click", () =>
      api.setFont((this.cfg?.font_px ?? 56) - 4),
    );
    this.el("font-up").addEventListener("click", () =>
      api.setFont((this.cfg?.font_px ?? 56) + 4),
    );
    this.el<HTMLSelectElement>("reading-font").addEventListener("change", (e) => {
      void api.setReadingFont((e.target as HTMLSelectElement).value);
    });
    this.el("zone-w-down").addEventListener("click", () => this.zone(-ZONE_STEP_PCT, 0));
    this.el("zone-w-up").addEventListener("click", () => this.zone(ZONE_STEP_PCT, 0));
    this.el("zone-h-down").addEventListener("click", () => this.zone(0, -ZONE_STEP_PCT));
    this.el("zone-h-up").addEventListener("click", () => this.zone(0, ZONE_STEP_PCT));
    this.el("lead-down").addEventListener("click", () => this.lead(-1));
    this.el("lead-up").addEventListener("click", () => this.lead(1));
    this.el("mirror-h").addEventListener("click", () =>
      api.setMirror(!(this.cfg?.mirror_h ?? false), this.cfg?.mirror_v ?? false),
    );
    this.el("mirror-v").addEventListener("click", () =>
      api.setMirror(this.cfg?.mirror_h ?? false, !(this.cfg?.mirror_v ?? false)),
    );
    this.el("fullscreen").addEventListener("click", () => void toggleFullscreen());
    this.el("present").addEventListener("click", () => this.onPresent());
    this.el("resume").addEventListener("click", () => this.onResume());
    this.el("settings").addEventListener("click", () => this.togglePanel());
    this.el<HTMLSelectElement>("model").addEventListener("change", (e) => {
      void api.setModel((e.target as HTMLSelectElement).value);
    });
    // Download buttons are re-rendered; delegate on the stable container.
    this.el("model-list").addEventListener("click", (e) => {
      const btn = (e.target as HTMLElement).closest("[data-model]");
      if (btn) void api.downloadModel(btn.getAttribute("data-model")!);
    });
  }

  togglePanel(open?: boolean): void {
    const panel = this.panel();
    const show = open ?? panel.classList.contains("hidden");
    panel.classList.toggle("hidden", !show);
    this.el("settings").classList.toggle("on", show);
  }

  /** Synchronous read-modify-write over the CACHED config — Alt+arrow key
   * repeats can't drop steps (the cache updates optimistically). */
  zone(dw: number, dh: number): void {
    if (!this.cfg) return;
    const width = zoneStep(this.cfg.reading_width, dw, ZONE_WIDTH_RANGE);
    const height = zoneStep(this.cfg.reading_height, dh, ZONE_HEIGHT_RANGE);
    this.cfg = { ...this.cfg, reading_width: width, reading_height: height };
    void api.setReadingZone(width, height);
  }

  lead(delta: number): void {
    if (!this.cfg) return;
    const lines = Math.max(0, Math.min(MAX_LEAD_LINES, this.cfg.lead_lines + delta));
    this.cfg = { ...this.cfg, lead_lines: lines };
    void api.setLead(lines);
  }

  /** (Re)populate the input-device dropdown; keeps the persisted choice
   * selected and falls back to system default when it disappeared. */
  async refreshDevices(): Promise<void> {
    const list = await api.listDevices().catch(() => null);
    const sel = this.el<HTMLSelectElement>("device");
    const want = this.cfg?.device ?? "";
    sel.textContent = "";
    const def = document.createElement("option");
    def.value = "";
    def.textContent = "default mic";
    sel.appendChild(def);
    for (const d of list ?? []) {
      const opt = document.createElement("option");
      opt.value = d.name;
      opt.textContent = d.default ? `${d.name} ✓` : d.name;
      sel.appendChild(opt);
    }
    sel.value = want;
    if (sel.value !== want) sel.value = "";
  }

  /** Feed the startup probe: model picker options + install states. */
  setModels(probe: StartupInfoPayload): void {
    this.models = probe.models;
    const sel = this.el<HTMLSelectElement>("model");
    sel.textContent = "";
    for (const m of probe.models) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      sel.appendChild(opt);
    }
    sel.value = this.cfg?.model ?? probe.model;
    this.renderModelList();
  }

  /** Live download progress for the model rows. */
  updateModelFetch(ev: ModelFetchEvent): void {
    this.activeFetch = ev;
    if (ev.phase === "ready") {
      const m = this.models.find((m) => m.id === ev.model);
      if (m) m.installed = true;
      this.activeFetch = null;
    }
    if (ev.phase === "cancelled" || ev.phase === "fatal") {
      this.activeFetch = null;
    }
    this.renderModelList();
  }

  private renderModelList(): void {
    const list = this.el("model-list");
    list.textContent = "";
    for (const m of this.models) {
      const row = document.createElement("div");
      row.className = "row model-row";
      const label = document.createElement("span");
      label.className = "label";
      label.textContent = `${m.label} · ${fmtMB(m.archive_bytes)}`;
      row.appendChild(label);
      const status = document.createElement("span");
      status.className = "cluster";
      if (m.installed) {
        status.innerHTML = `<span class="installed">✓ installed</span>`;
      } else if (this.activeFetch && this.activeFetch.model === m.id) {
        const f = this.activeFetch;
        status.innerHTML =
          f.phase === "extracting"
            ? `<span class="progress">extracting…</span>`
            : `<span class="progress">${f.pct}%</span>`;
      } else {
        const btn = document.createElement("button");
        btn.setAttribute("data-model", m.id);
        btn.textContent = "Download";
        btn.disabled = this.activeFetch !== null;
        status.appendChild(btn);
      }
      row.appendChild(status);
      list.appendChild(row);
    }
  }

  applyConfig(cfg: GuiConfig): void {
    this.cfg = cfg;
    this.el("mode").textContent = cfg.scroll_mode;
    this.el("wpm").textContent = String(cfg.wpm);
    this.el("font-px").textContent = String(cfg.font_px);
    this.el("lead").textContent = String(cfg.lead_lines);
    this.el("zone-w").textContent = String(cfg.reading_width);
    this.el("zone-h").textContent = String(cfg.reading_height);
    this.el<HTMLSelectElement>("reading-font").value = cfg.reading_font;
    this.el("mirror-h").classList.toggle("on", cfg.mirror_h);
    this.el("mirror-v").classList.toggle("on", cfg.mirror_v);
    const dumb = cfg.scroll_mode === "dumb";
    this.el("wpm-group").classList.toggle("hidden", !dumb);
    this.el("play").classList.toggle("hidden", !dumb);
    this.el("start").classList.toggle("hidden", dumb);
    this.el("device").classList.toggle("hidden", dumb);
    const dev = this.el<HTMLSelectElement>("device");
    if (dev.value !== cfg.device) dev.value = cfg.device;
    const model = this.el<HTMLSelectElement>("model");
    if (model.value !== cfg.model) model.value = cfg.model;
  }

  applyStatus(status: StatusPayload): void {
    const pill = document.getElementById("pill")!;
    this.el("pill-label").textContent =
      status.state === "listening" ? "listening" : status.state;
    // The message (model path, device failure…) is readable on hover.
    pill.title = status.message;
    if (!status.running) {
      pill.className = status.state === "error" ? "error" : "";
    }
    this.el("start").textContent = status.running ? "Stop" : "Listen";
    const startBtn = this.el("start");
    startBtn.onclick = () => void (status.running ? api.stop() : api.start());
    // Device/model changes need a session restart; lock them while live.
    this.el<HTMLSelectElement>("device").disabled = status.running;
    this.el<HTMLSelectElement>("model").disabled = status.running;
  }

  applyTrack(state: TrackState): void {
    const pill = document.getElementById("pill")!;
    pill.className = state.toLowerCase();
    this.el("pill-label").textContent = state.toLowerCase();
  }

  setPlaying(playing: boolean): void {
    this.playing = playing;
    this.el("play").textContent = playing ? "⏸" : "▶";
  }

  setFollowSuspended(suspended: boolean): void {
    this.el("resume").classList.toggle("hidden", !suspended);
  }

  setChromeHidden(hidden: boolean): void {
    this.chromeHidden = hidden;
    this.applyVisibility();
  }

  private touch(): void {
    this.lastActivity = performance.now();
    this.applyVisibility();
  }

  /** THE single write site for chrome visibility. Presentation-hide is
   * absolute: while chromeHidden, no other input can reveal the overlay. */
  private applyVisibility(): void {
    document.body.classList.toggle("chrome-off", this.chromeHidden);
    const idle = performance.now() - this.lastActivity > AUTO_HIDE_MS;
    const faded = this.chromeHidden || idle;
    document.getElementById("controls")?.classList.toggle("faded", faded);
    this.panel().classList.toggle("faded", faded);
  }
}
