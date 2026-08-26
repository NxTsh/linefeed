// Splash phase machine. The brand beat is deliberate: even instant cached
// startups hold the splash for MIN_DWELL_MS; slow phases (model load, model
// download) extend past it. Monotone by rank — a stale event can never move
// the splash backwards.

export const MIN_DWELL_MS = 3000;
export const FADE_MS = 300;

export type SplashPhase =
  | "booting"
  | "engine-ready"
  | "restoring-script"
  | "script-loaded"
  | "done";

const RANK: Record<SplashPhase, number> = {
  booting: 0,
  "engine-ready": 1,
  "restoring-script": 2,
  "script-loaded": 3,
  done: 4,
};

export const PHASE_COPY: Record<SplashPhase, string> = {
  booting: "starting…",
  "engine-ready": "engine ready",
  "restoring-script": "restoring last script…",
  "script-loaded": "script loaded",
  done: "",
};

/** Advance only forward. */
export function advancePhase(cur: SplashPhase, next: SplashPhase): SplashPhase {
  return RANK[next] > RANK[cur] ? next : cur;
}

export interface SplashGate {
  phase: SplashPhase;
  /** A model fetch (offer or download) holds the splash open. */
  fetchHold: boolean;
  /** A fatal engine/model error replaces dismissal with the error view. */
  error: boolean;
}

export function shouldDismiss(g: SplashGate): boolean {
  return g.phase === "done" && !g.fetchHold && !g.error;
}

/** Absolute time the splash may dismiss: everything ready AND the minimum
 * dwell served. dwell = 0 disables the beat (tests). */
export function dismissAt(
  startMs: number,
  readyMs: number,
  dwellMs: number = MIN_DWELL_MS,
): number {
  return Math.max(readyMs, startMs + dwellMs);
}
