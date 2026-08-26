// Presentation mode: `h` hides EVERY pixel of chrome. While hidden, mouse
// movement can never reveal — only `h`, `Esc`, or a deliberate ≥500 ms rest
// in the bottom 8% edge band, and only after the pointer has been seen
// OUTSIDE the band first (so a cursor parked on the eye button when chrome
// hides can't re-reveal it). Pure state machine; DOM application lives in
// controls.ts through ONE visibility funnel.

export const EDGE_BAND = 0.08;
export const EDGE_MS = 500;
/** Overlay auto-hide while chrome is visible but idle. */
export const AUTO_HIDE_MS = 3000;
export const HINT_MS = 1500;
export const PRESENT_HINT =
  "h = show menu · ⌘D/Ctrl+D = debug log · f = resume follow";

export interface PresentState {
  hidden: boolean;
  /** Timestamp when the pointer entered the edge band (while hidden). */
  edgeArmedAt: number | null;
  /** The pointer has been seen outside the band since chrome hid. */
  seenOutsideBand: boolean;
}

export function presentInit(): PresentState {
  return { hidden: false, edgeArmedAt: null, seenOutsideBand: false };
}

export function hideChrome(_st: PresentState): PresentState {
  return { hidden: true, edgeArmedAt: null, seenOutsideBand: false };
}

export function revealChrome(_st: PresentState): PresentState {
  return { hidden: false, edgeArmedAt: null, seenOutsideBand: false };
}

export function toggleHidden(st: PresentState): PresentState {
  return st.hidden ? revealChrome(st) : hideChrome(st);
}

/** Pointer moved to `yFrac` (0 = top, 1 = bottom) at time `now` (ms).
 * Returns the new state plus whether chrome should reveal. Sweeping
 * through the band never reveals — only resting in it does. */
export function pointerMove(
  st: PresentState,
  yFrac: number,
  now: number,
): { st: PresentState; reveal: boolean } {
  if (!st.hidden) return { st, reveal: false };
  const inBand = yFrac >= 1 - EDGE_BAND;
  if (!inBand) {
    return {
      st: { ...st, edgeArmedAt: null, seenOutsideBand: true },
      reveal: false,
    };
  }
  if (!st.seenOutsideBand) return { st, reveal: false };
  if (st.edgeArmedAt === null) {
    return { st: { ...st, edgeArmedAt: now }, reveal: false };
  }
  // Still armed; the tick decides. Movement WITHIN the band keeps the arm.
  return { st, reveal: false };
}

/** Periodic tick while hidden: reveal once the pointer has rested in the
 * band for EDGE_MS. */
export function edgePeekTick(
  st: PresentState,
  now: number,
): { st: PresentState; reveal: boolean } {
  if (!st.hidden || st.edgeArmedAt === null) return { st, reveal: false };
  if (now - st.edgeArmedAt >= EDGE_MS) {
    return { st: revealChrome(st), reveal: true };
  }
  return { st, reveal: false };
}
