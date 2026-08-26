// Reading-zone arithmetic: a centered box, sized in percent of the stage.
// Clamps mirror Rust's config sanitize (contract-tested).

export const ZONE_WIDTH_RANGE: [number, number] = [40, 100];
export const ZONE_HEIGHT_RANGE: [number, number] = [30, 100];
export const ZONE_STEP_PCT = 5;

export function clampZone(width: number, height: number): { width: number; height: number } {
  const [wLo, wHi] = ZONE_WIDTH_RANGE;
  const [hLo, hHi] = ZONE_HEIGHT_RANGE;
  return {
    width: Math.max(wLo, Math.min(wHi, Math.round(width))),
    height: Math.max(hLo, Math.min(hHi, Math.round(height))),
  };
}

export function zoneStep(value: number, delta: number, range: [number, number]): number {
  const [lo, hi] = range;
  return Math.max(lo, Math.min(hi, value + delta));
}

/** Centered rect of the zone inside a stage of the given pixel size. */
export function zoneBox(
  stageW: number,
  stageH: number,
  widthPct: number,
  heightPct: number,
): { x: number; y: number; w: number; h: number } {
  const w = (stageW * widthPct) / 100;
  const h = (stageH * heightPct) / 100;
  return { x: (stageW - w) / 2, y: (stageH - h) / 2, w, h };
}
