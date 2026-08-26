// The only wiring file: splash boot → probe → optional model fetch →
// restore script → subscriptions → keyboard.

import "./styles.css";
import {
  api,
  hasTauri,
  onConfig,
  onDumb,
  onModelFetch,
  onStatus,
  onTracker,
  pickScript,
  toggleFullscreen,
} from "./api.ts";
import { Controls } from "./controls.ts";
import {
  fetchHoldsSplash,
  fetchStatusLine,
  isTerminal,
  nextFetch,
  shouldOfferFetch,
  type FetchUiState,
} from "./model-fetch.ts";
import {
  edgePeekTick,
  hideChrome,
  pointerMove,
  presentInit,
  revealChrome,
  HINT_MS,
  PRESENT_HINT,
  type PresentState,
} from "./present.ts";
import { PrompterView } from "./prompter.ts";
import {
  advancePhase,
  dismissAt,
  FADE_MS,
  PHASE_COPY,
  shouldDismiss,
  type SplashPhase,
} from "./splash.ts";
import type { ModelFetchEvent, StartupInfoPayload } from "./types.ts";

const splashStart = performance.now();

function el(id: string): HTMLElement {
  return document.getElementById(id)!;
}

function hud(msg: string, ms = HINT_MS): void {
  const h = el("hud");
  h.textContent = msg;
  h.classList.add("show");
  window.setTimeout(() => h.classList.remove("show"), ms);
}

async function main(): Promise<void> {
  const view = new PrompterView();
  const controls = new Controls(el("controls-root"));

  // ---- splash state ------------------------------------------------------
  let phase: SplashPhase = "booting";
  let fetchState: FetchUiState = "absent";
  let lastFetchEvent: ModelFetchEvent | null = null;
  let splashError = false;
  const setPhase = (p: SplashPhase): void => {
    phase = advancePhase(phase, p);
    const copy = PHASE_COPY[phase];
    if (copy) el("splash-phase").textContent = copy;
  };
  const maybeDismissSplash = (): void => {
    if (!shouldDismiss({ phase, fetchHold: fetchHoldsSplash(fetchState), error: splashError })) {
      return;
    }
    const at = dismissAt(splashStart, performance.now());
    window.setTimeout(() => {
      const splash = el("splash");
      splash.classList.add("fading");
      window.setTimeout(() => splash.classList.add("gone"), FADE_MS);
    }, Math.max(0, at - performance.now()));
  };

  // ---- presentation mode -------------------------------------------------
  let present: PresentState = presentInit();
  const applyPresent = (): void => controls.setChromeHidden(present.hidden);
  document.addEventListener("pointermove", (e) => {
    const res = pointerMove(present, e.clientY / window.innerHeight, performance.now());
    present = res.st;
    if (res.reveal) applyPresent();
  });
  window.setInterval(() => {
    const res = edgePeekTick(present, performance.now());
    if (res.reveal || res.st !== present) {
      present = res.st;
      if (res.reveal) applyPresent();
    }
  }, 100);

  // ---- subscriptions -----------------------------------------------------
  let scriptLoaded = false;
  const openScript = async (path: string): Promise<void> => {
    const payload = await api.loadScript(path);
    if (payload) {
      view.setScript(payload);
      scriptLoaded = true;
    }
  };
  controls.onOpen = (p) => void openScript(p);
  controls.onPresent = () => {
    present = hideChrome(present);
    applyPresent();
    hud(PRESENT_HINT);
  };
  controls.onResume = () => view.resumeFollow();
  view.onFollowChange = (s) => controls.setFollowSuspended(s);

  onConfig((cfg) => {
    view.applyConfig(cfg);
    controls.applyConfig(cfg);
  });
  onStatus((s) => {
    controls.applyStatus(s);
    if (s.state === "error") {
      if (scriptLoaded) {
        // Post-boot errors get a readable toast, not just a red dot.
        hud(`error: ${s.message}`, 6000);
      } else {
        // Pre-script engine errors surface on the splash.
        splashError = true;
        el("error-msg").textContent = s.message;
        el("error-box").classList.remove("hidden");
      }
    }
    if (s.state === "stopped" || s.state === "error") {
      // Devices may have changed (unplugged interface, new permission).
      void controls.refreshDevices();
    }
  });
  onTracker((ev) => {
    view.setCursor(ev.cursor);
    controls.applyTrack(ev.state);
    el("banner").classList.toggle("show", ev.state === "LOST");
    el("posbar").style.width = `${Math.min(100, (ev.cursor / Math.max(1, totalTokens)) * 100)}%`;
  });
  onDumb((ev) => {
    view.setCursor(ev.cursor);
    controls.setPlaying(ev.playing);
    el("posbar").style.width = `${Math.min(100, (ev.cursor / Math.max(1, totalTokens)) * 100)}%`;
  });

  // ---- model fetch UI ----------------------------------------------------
  const fetchBox = el("fetch-box");
  const applyFetch = (ev: ModelFetchEvent | null): void => {
    lastFetchEvent = ev;
    el("fetch-status").textContent = fetchStatusLine(fetchState, ev);
    if (ev && ev.total > 0) el("fetch-bar").style.width = `${ev.pct}%`;
    if (fetchState === "fatal" && ev?.curl) {
      const curl = el("fetch-curl");
      curl.textContent = ev.curl;
      curl.classList.remove("hidden");
    }
    if (isTerminal(fetchState)) {
      window.setTimeout(() => fetchBox.classList.add("hidden"), 1200);
      maybeDismissSplash();
    }
  };
  onModelFetch((ev) => {
    fetchState = nextFetch(fetchState, ev);
    applyFetch(ev);
    controls.updateModelFetch(ev);
    if (ev.phase === "ready") {
      // Refresh install states in the settings panel.
      void api.startupProbe().then((p) => p && controls.setModels(p));
    }
  });
  el("fetch-accept").addEventListener("click", () => {
    if (fetchState !== "offer") return;
    fetchState = "downloading";
    applyFetch(lastFetchEvent);
    void api.downloadModel();
  });
  el("fetch-decline").addEventListener("click", () => {
    if (fetchState === "offer") {
      fetchState = "declined";
    } else if (!isTerminal(fetchState)) {
      void api.cancelModelFetch();
      return; // the cancelled event closes the box
    }
    applyFetch(lastFetchEvent);
  });
  el("error-continue").addEventListener("click", () => {
    splashError = false;
    el("error-box").classList.add("hidden");
    maybeDismissSplash();
  });

  // ---- boot --------------------------------------------------------------
  let totalTokens = 1;
  if (hasTauri()) {
    const [cfg, probe] = await Promise.all([api.getConfig(), api.startupProbe()]);
    if (cfg) {
      view.applyConfig(cfg);
      controls.applyConfig(cfg);
    }
    void controls.refreshDevices();
    if (probe) controls.setModels(probe);
    setPhase("engine-ready");
    if (probe && shouldOfferFetch(probe)) {
      fetchState = "offer";
      // Show the exact install dir so an env-var mismatch (download went
      // to a different models dir than the one being probed) is visible.
      el("fetch-msg").textContent =
        `The pt-BR speech model (~131 MB) is not installed in ` +
        `${probe.models_dir}. Download it now? Voice-following needs it; ` +
        `dumb scroll works without it.`;
      fetchBox.classList.remove("hidden");
    } else if (probe && !probe.models_ok && !probe.fetchable) {
      splashError = true;
      el("error-msg").textContent =
        `models missing in ${probe.models_dir}: ${probe.missing.join(", ")} — ` +
        "voice-following is unavailable; dumb scroll still works.";
      el("error-box").classList.remove("hidden");
    }
    const last = (probe as StartupInfoPayload | null)?.last_script;
    if (last) {
      setPhase("restoring-script");
      await openScript(last);
      setPhase("script-loaded");
    }
  }
  setPhase("done");
  maybeDismissSplash();

  // Track token totals for the progress bar.
  const origSetScript = view.setScript.bind(view);
  view.setScript = (p) => {
    totalTokens = Math.max(1, p.n_tokens);
    origSetScript(p);
  };

  // ---- keyboard ----------------------------------------------------------
  const fetchKeys = (e: KeyboardEvent): boolean => {
    if (fetchBox.classList.contains("hidden")) return false;
    if (e.key === "Enter") {
      el("fetch-accept").click();
      return true;
    }
    if (e.key === "Escape") {
      el("fetch-decline").click();
      return true;
    }
    return false;
  };
  // Capture phase: fetch-box keys never leak to the prompter.
  document.addEventListener(
    "keydown",
    (e) => {
      if (fetchKeys(e)) {
        e.preventDefault();
        e.stopPropagation();
      }
    },
    { capture: true },
  );

  document.addEventListener("keydown", async (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      const p = await pickScript();
      if (p) void openScript(p);
    } else if (mod && e.key.toLowerCase() === "d") {
      e.preventDefault();
      const cfg = await api.getConfig();
      const next = !(cfg?.debug_log ?? false);
      await api.setDebugLog(next);
      hud(next ? "diagnostics ON from next session" : "diagnostics OFF");
    } else if (mod && e.key.toLowerCase() === "f") {
      e.preventDefault();
      void toggleFullscreen();
    } else if (e.key === "F11") {
      e.preventDefault();
      void toggleFullscreen();
    } else if (e.key === "h" && !mod) {
      present = present.hidden ? revealChrome(present) : hideChrome(present);
      applyPresent();
      if (present.hidden) hud(PRESENT_HINT);
    } else if (e.key === "Escape") {
      if (present.hidden) {
        present = revealChrome(present);
        applyPresent();
      }
    } else if (e.key === "f" && !mod) {
      view.resumeFollow();
    } else if (e.key === " ") {
      // Space is dumb-mode only: during a voice session it must NOT
      // silently switch modes.
      e.preventDefault();
      const cfg = await api.getConfig();
      if (cfg?.scroll_mode === "dumb") {
        const play = el("controls-root").querySelector('[data-c="play"]') as HTMLElement;
        play.click();
      }
    } else if (e.key === "[") {
      controls.lead(-1);
    } else if (e.key === "]") {
      controls.lead(1);
    } else if (e.altKey && e.key === "ArrowLeft") {
      controls.zone(-5, 0);
    } else if (e.altKey && e.key === "ArrowRight") {
      controls.zone(5, 0);
    } else if (e.altKey && e.key === "ArrowUp") {
      controls.zone(0, 5);
    } else if (e.altKey && e.key === "ArrowDown") {
      controls.zone(0, -5);
    }
  });

  // Debounced relayout on resize + correct metrics once fonts load.
  let resizeTimer = 0;
  window.addEventListener("resize", () => {
    window.clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => {
      view.markLayoutDirty();
      view.reanchor();
    }, 150);
  });
  void document.fonts?.ready.then(() => {
    view.markLayoutDirty();
    view.reanchor();
  });
}

void main().catch((e) => {
  console.error("linefeed boot failed:", e);
  el("splash-phase").textContent = `boot failed: ${e}`;
});
