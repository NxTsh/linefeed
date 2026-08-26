import { test } from "node:test";
import assert from "node:assert/strict";
import {
  edgePeekTick,
  hideChrome,
  pointerMove,
  presentInit,
  revealChrome,
  toggleHidden,
  EDGE_BAND,
  EDGE_MS,
  PRESENT_HINT,
} from "../src/present.ts";

test("initial state is visible", () => {
  assert.equal(presentInit().hidden, false);
});

test("h toggles, Esc only reveals", () => {
  let st = presentInit();
  st = toggleHidden(st);
  assert.equal(st.hidden, true);
  st = toggleHidden(st);
  assert.equal(st.hidden, false);
  st = revealChrome(hideChrome(st));
  assert.equal(st.hidden, false);
});

test("mouse movement can NEVER reveal while hidden (outside band)", () => {
  let st = hideChrome(presentInit());
  for (const y of [0.1, 0.5, 0.85, 0.2, 0.9, 0.4]) {
    const r = pointerMove(st, y, 1000);
    st = r.st;
    assert.equal(r.reveal, false);
    assert.equal(st.hidden, true);
  }
});

test("parked cursor in the band never reveals (live-bug regression)", () => {
  // Chrome hides while the pointer rests on the eye button (inside the
  // band). Without moving OUTSIDE first, dwelling must not reveal.
  let st = hideChrome(presentInit());
  const inBand = 1 - EDGE_BAND / 2;
  let r = pointerMove(st, inBand, 0);
  st = r.st;
  r = pointerMove(st, inBand, 100);
  st = r.st;
  const tick = edgePeekTick(st, 10_000);
  assert.equal(tick.reveal, false, "never armed without leaving the band");
});

test("deliberate rest in the band reveals after EDGE_MS", () => {
  let st = hideChrome(presentInit());
  st = pointerMove(st, 0.5, 0).st; // seen outside the band
  st = pointerMove(st, 0.97, 100).st; // enters band, arms
  assert.equal(edgePeekTick(st, 100 + EDGE_MS - 1).reveal, false, "499ms: not yet");
  const done = edgePeekTick(st, 100 + EDGE_MS);
  assert.equal(done.reveal, true, "500ms dwell reveals");
  assert.equal(done.st.hidden, false);
});

test("jitter WITHIN the band keeps the arm", () => {
  let st = hideChrome(presentInit());
  st = pointerMove(st, 0.5, 0).st;
  st = pointerMove(st, 0.95, 100).st;
  st = pointerMove(st, 0.99, 300).st; // still in band — arm time unchanged
  assert.equal(edgePeekTick(st, 100 + EDGE_MS).reveal, true);
});

test("a pass-through sweep does not reveal", () => {
  let st = hideChrome(presentInit());
  st = pointerMove(st, 0.5, 0).st;
  st = pointerMove(st, 0.97, 100).st; // enter band
  st = pointerMove(st, 0.5, 200).st; // leave before EDGE_MS — disarms
  assert.equal(edgePeekTick(st, 5000).reveal, false);
});

test("band is the bottom 8 percent", () => {
  assert.equal(EDGE_BAND, 0.08);
  let st = hideChrome(presentInit());
  st = pointerMove(st, 0.5, 0).st;
  st = pointerMove(st, 1 - EDGE_BAND - 0.001, 100).st;
  assert.equal(st.edgeArmedAt, null, "just above the band does not arm");
  st = pointerMove(st, 1 - EDGE_BAND + 0.001, 200).st;
  assert.notEqual(st.edgeArmedAt, null, "inside the band arms");
});

test("hide resets the outside-band gate", () => {
  let st = hideChrome(presentInit());
  st = pointerMove(st, 0.5, 0).st;
  st = revealChrome(st);
  st = hideChrome(st);
  assert.equal(st.seenOutsideBand, false, "each hide requires a fresh exit");
});

test("hint copy mentions the escape hatches", () => {
  assert.ok(PRESENT_HINT.includes("h = show menu"));
  assert.ok(PRESENT_HINT.includes("f = resume follow"));
});
