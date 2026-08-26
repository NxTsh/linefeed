import { test } from "node:test";
import assert from "node:assert/strict";
import {
  groupVisualRows,
  rowOfWordTable,
  rowPitch,
  visualTolerance,
  type WordMeasure,
} from "../src/visual.ts";

function m(idx: number, y: number, h = 40, para = 0): WordMeasure {
  return { idx, y, h, para };
}

test("groups words by measured y", () => {
  const rows = groupVisualRows([m(0, 0), m(1, 0), m(2, 58), m(3, 58), m(4, 116)], 4);
  assert.equal(rows.length, 3);
  assert.deepEqual(
    rows.map((r) => [r.firstWord, r.lastWord]),
    [
      [0, 1],
      [2, 3],
      [4, 4],
    ],
  );
});

test("sub-pixel jitter merges; distinct rows split", () => {
  const rows = groupVisualRows([m(0, 0), m(1, 1.4), m(2, 40)], 2);
  assert.equal(rows.length, 2);
  assert.equal(rows[0]!.lastWord, 1);
});

test("paragraph boundary forces a new row even within tolerance", () => {
  const rows = groupVisualRows([m(0, 0, 40, 0), m(1, 1, 40, 1)], 4);
  assert.equal(rows.length, 2, "same y but different paragraphs");
});

test("row height spans max bottom minus top", () => {
  const rows = groupVisualRows([m(0, 10, 40), m(1, 12, 44)], 4);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]!.top, 10);
  assert.equal(rows[0]!.height, 12 + 44 - 10);
});

test("tolerance: 30% of median pitch clamped to [1, 12]", () => {
  assert.equal(visualTolerance([]), 4, "default without pitches");
  assert.equal(visualTolerance([2, 2, 2]), 1, "lower clamp");
  assert.equal(visualTolerance([100, 100]), 12, "upper clamp");
  const mid = visualTolerance([20, 20, 20]);
  assert.ok(Math.abs(mid - 6) < 1e-9);
});

test("rowOfWordTable and rowPitch", () => {
  const rows = groupVisualRows([m(0, 0), m(1, 0), m(2, 60), m(3, 120)], 4);
  const table = rowOfWordTable(rows, 4);
  assert.deepEqual(table, [0, 0, 1, 2]);
  assert.equal(rowPitch(rows), 60);
  assert.equal(rowPitch([]), 0);
  assert.equal(rowPitch(rows.slice(0, 1)), rows[0]!.height, "single row falls back to height");
});

test("empty input", () => {
  assert.deepEqual(groupVisualRows([], 4), []);
  assert.deepEqual(rowOfWordTable([], 0), []);
});
