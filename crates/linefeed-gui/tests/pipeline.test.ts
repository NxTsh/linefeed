import { test } from "node:test";
import assert from "node:assert/strict";
import {
  anchorY,
  currentWordIndex,
  intraRowFraction,
  mirrorTransform,
  progress,
  scrollTarget,
  ANCHOR_FRACTION,
  MIN_LOOKAHEAD_LINES,
  READING_FONTS,
} from "../src/pipeline.ts";
import type { ScriptPayload, WordPayload } from "../src/types.ts";

function words(spans: [number, number][]): WordPayload[] {
  return spans.map(([ts, te], i) => ({ raw: `w${i}`, ts, te, line: 0, para: 0 }));
}

test("currentWordIndex: basics and digit multi-token words", () => {
  // "tenho 23 anos" → spans [0,1) [1,4) [4,5)
  const ws = words([
    [0, 1],
    [1, 4],
    [4, 5],
  ]);
  assert.equal(currentWordIndex(ws, 0), -1, "nothing read yet");
  assert.equal(currentWordIndex(ws, 1), 0);
  assert.equal(currentWordIndex(ws, 2), 1, "mid-digit-word");
  assert.equal(currentWordIndex(ws, 4), 1, "end of digit word");
  assert.equal(currentWordIndex(ws, 5), 2);
  assert.equal(currentWordIndex(ws, 99), 2, "past the end clamps");
  assert.equal(currentWordIndex([], 3), -1);
});

test("anchorY: base third with enough room", () => {
  const a = anchorY(900, 50);
  assert.equal(a, 900 * ANCHOR_FRACTION);
});

test("anchorY: rises to keep MIN_LOOKAHEAD_LINES visible", () => {
  for (const lineH of [40, 80, 120, 160]) {
    const zoneH = 600;
    const a = anchorY(zoneH, lineH);
    const below = zoneH - a;
    if (a > lineH / 2) {
      assert.ok(
        below >= (1 + MIN_LOOKAHEAD_LINES) * lineH - 1e-6 ||
          below >= 2.1 * lineH - 1e-6,
        `lineH=${lineH}: below=${below}`,
      );
    }
  }
});

test("anchorY: tiny zone + huge font degrades without clipping", () => {
  const a = anchorY(300, 200);
  assert.ok(a >= 100, `current row never clipped: ${a}`);
  assert.equal(a, 100, "floor at lineH/2");
});

test("anchorY: zero sizes are safe", () => {
  assert.equal(anchorY(0, 50), 0);
  assert.equal(anchorY(500, 0), 0);
});

test("scrollTarget: lead=0 frac=0 is the strict line lock", () => {
  const tops = [0, 60, 120, 180, 240];
  assert.equal(scrollTarget(tops, 2, 0, 0, 40, 1000), 120 - 40);
});

test("scrollTarget: lead shifts down, clamped at last row", () => {
  const tops = [0, 60, 120, 180, 240];
  assert.equal(scrollTarget(tops, 2, 0, 1, 40, 1000), 180 - 40);
  assert.equal(scrollTarget(tops, 4, 0, 3, 0, 1000), 240, "lead clamps at last");
});

test("scrollTarget: intra-row fraction glides continuously", () => {
  const tops = [0, 60, 120];
  const at0 = scrollTarget(tops, 0, 0, 0, 0, 1000);
  const atHalf = scrollTarget(tops, 0, 0.5, 0, 0, 1000);
  const at1 = scrollTarget(tops, 1, 0, 0, 0, 1000);
  assert.equal(at0, 0);
  assert.equal(atHalf, 30);
  assert.equal(at1, 60);
  // Monotone smoke across a fine sweep.
  let prev = -1;
  for (let f = 0; f <= 1; f += 0.05) {
    const y = scrollTarget(tops, 1, f, 0, 0, 1000);
    assert.ok(y >= prev, "monotone glide");
    prev = y;
  }
});

test("scrollTarget: clamped to [0, maxTarget] and empty-safe", () => {
  const tops = [0, 60, 120];
  assert.equal(scrollTarget(tops, 0, 0, 0, 500, 1000), 0, "never negative");
  assert.equal(scrollTarget(tops, 2, 0, 0, 0, 50), 50, "max clamp");
  assert.equal(scrollTarget([], 0, 0, 1, 0, 100), 0);
});

test("intraRowFraction over token spans", () => {
  // Two rows: words 0-1 on row 0 (tokens 0..4), word 2 on row 1.
  const ws = words([
    [0, 2],
    [2, 4],
    [4, 6],
  ]);
  const rowOf = (w: number) => (w <= 1 ? 0 : 1);
  assert.equal(intraRowFraction(ws, rowOf, 0), 0);
  assert.equal(intraRowFraction(ws, rowOf, 2), 0.5, "half the row's tokens read");
  assert.ok(intraRowFraction(ws, rowOf, 4) >= 0.999 - 1e-9, "row consumed caps below 1");
  assert.equal(intraRowFraction(ws, rowOf, 5), 0.5, "second row half");
});

test("mirrorTransform", () => {
  assert.equal(mirrorTransform(false, false, 120), "translateY(-120px)");
  assert.equal(mirrorTransform(true, false, 0), "scale(-1, 1) translateY(0px)");
  assert.equal(mirrorTransform(false, true, 10), "scale(1, -1) translateY(-10px)");
  assert.equal(mirrorTransform(true, true, 10), "scale(-1, -1) translateY(-10px)");
});

test("progress", () => {
  const script = { n_tokens: 200 } as ScriptPayload;
  assert.equal(progress(script, 50), 0.25);
  assert.equal(progress(script, 999), 1);
  assert.equal(progress(null, 10), 0);
  assert.equal(progress({ n_tokens: 0 } as ScriptPayload, 10), 0);
});

test("reading fonts: six entries, brand mono not offered", () => {
  assert.equal(READING_FONTS.length, 6);
  assert.ok(!READING_FONTS.some((f) => /sauce|mono/i.test(f.css)));
  assert.equal(READING_FONTS[0]!.id, "inter", "inter is the default face");
});
