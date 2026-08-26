// Pure, DOM-free render math: cursor → word → line → scroll target.
// All positioning consumes MEASURED geometry (visual.ts provides it); this
// module never touches the DOM.

import type { ScriptPayload, WordPayload } from "./types.ts";

/** Reading anchor: the current line sits a third of the way down the zone. */
export const ANCHOR_FRACTION = 1 / 3;
/** The zone below the anchor must show this many full unread lines… */
export const MIN_LOOKAHEAD_LINES = 3;
/** …or degrade to one full line plus this fraction of the next. */
export const PARTIAL_LOOKAHEAD_FRACTION = 0.1;
export const DEFAULT_LEAD_LINES = 1;
export const MAX_LEAD_LINES = 3;

/** The reading-font dropdown; ids mirror Rust's READING_FONT_IDS
 * (contract-tested). Brand mono is deliberately NOT offered for reading. */
export const READING_FONTS: { id: string; label: string; css: string }[] = [
  { id: "inter", label: "Inter", css: '"Inter", sans-serif' },
  {
    id: "atkinson",
    label: "Atkinson Hyperlegible",
    css: '"Atkinson Hyperlegible", sans-serif',
  },
  { id: "source-sans-3", label: "Source Sans 3", css: '"Source Sans 3", sans-serif' },
  { id: "noto-sans", label: "Noto Sans", css: '"Noto Sans", sans-serif' },
  { id: "georgia", label: "Georgia", css: "Georgia, serif" },
  { id: "system", label: "System", css: "system-ui, sans-serif" },
];

/** Index of the word containing token `cursor − 1` (the word being read).
 * −1 while nothing has been read. Binary search over token spans. */
export function currentWordIndex(words: WordPayload[], cursor: number): number {
  if (cursor <= 0 || words.length === 0) return -1;
  const tok = Math.min(cursor - 1, words[words.length - 1]!.te - 1);
  let lo = 0;
  let hi = words.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (words[mid]!.te <= tok) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** Fraction [0, 1) of the given visual row already read, measured in
 * tokens — this is what makes the scroll target glide continuously instead
 * of stepping once per row. */
export function intraRowFraction(
  words: WordPayload[],
  rowOfWord: (wordIdx: number) => number,
  cursor: number,
): number {
  const wi = currentWordIndex(words, cursor);
  if (wi < 0) return 0;
  const row = rowOfWord(wi);
  let first = wi;
  while (first > 0 && rowOfWord(first - 1) === row) first--;
  let last = wi;
  while (last + 1 < words.length && rowOfWord(last + 1) === row) last++;
  const rowStartTok = words[first]!.ts;
  const rowEndTok = words[last]!.te;
  if (rowEndTok <= rowStartTok) return 0;
  const read = Math.min(cursor, rowEndTok) - rowStartTok;
  return Math.max(0, Math.min(0.999, read / (rowEndTok - rowStartTok)));
}

/** Anchor y inside the zone: ⅓ down, rising when the space below can't hold
 * the current line + MIN_LOOKAHEAD_LINES unread lines; degrades to
 * 1 + PARTIAL lines on tiny zones / huge fonts; never clips the current
 * line (floor at lineH/2). */
export function anchorY(zoneH: number, lineH: number): number {
  if (zoneH <= 0 || lineH <= 0) return 0;
  const base = zoneH * ANCHOR_FRACTION;
  const fullNeed = (1 + MIN_LOOKAHEAD_LINES) * lineH;
  let a = Math.min(base, zoneH - fullNeed);
  if (a < lineH / 2) {
    const partialNeed = (2 + PARTIAL_LOOKAHEAD_FRACTION) * lineH;
    a = Math.min(base, zoneH - partialNeed);
    if (a < lineH / 2) a = lineH / 2;
  }
  return a;
}

/** Scroll target: the (row + lead) top glides to the anchor, interpolated
 * by the intra-row fraction. lead = 0 and frac = 0 reproduce the strict
 * line lock. Clamped to [0, maxTarget]. */
export function scrollTarget(
  rowTops: number[],
  rowIdx: number,
  frac: number,
  lead: number,
  anchor: number,
  maxTarget: number,
): number {
  if (rowTops.length === 0) return 0;
  const last = rowTops.length - 1;
  const eff = Math.min(Math.max(rowIdx, 0) + Math.max(lead, 0), last);
  const top = rowTops[eff]!;
  const next = rowTops[Math.min(eff + 1, last)]!;
  const y = top + (next - top) * Math.max(0, Math.min(1, frac));
  return Math.max(0, Math.min(y - anchor, Math.max(0, maxTarget)));
}

/** Inline transform for the scroll column. Mirror scale is applied BEFORE
 * the translate so spring pixels stay unmirrored. */
export function mirrorTransform(h: boolean, v: boolean, translateY: number): string {
  const sx = h ? -1 : 1;
  const sy = v ? -1 : 1;
  const scale = h || v ? `scale(${sx}, ${sy}) ` : "";
  return `${scale}translateY(${-translateY}px)`;
}

/** Progress through the script in [0, 1]. */
export function progress(script: ScriptPayload | null, cursor: number): number {
  if (!script || script.n_tokens === 0) return 0;
  return Math.max(0, Math.min(1, cursor / script.n_tokens));
}
