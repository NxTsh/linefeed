import { test } from "node:test";
import assert from "node:assert/strict";
import {
  effectiveTarget,
  followInit,
  followResume,
  followSuspend,
  followWheel,
} from "../src/follow.ts";

test("suspend captures the current position", () => {
  const st = followSuspend(followInit(), 240);
  assert.equal(st.suspended, true);
  assert.equal(st.manualTarget, 240);
});

test("wheel accumulates and clamps", () => {
  let st = followSuspend(followInit(), 100);
  st = followWheel(st, 50, 1000);
  assert.equal(st.manualTarget, 150);
  st = followWheel(st, -500, 1000);
  assert.equal(st.manualTarget, 0, "floor clamp");
  st = followWheel(st, 5000, 1000);
  assert.equal(st.manualTarget, 1000, "ceiling clamp");
});

test("negative maxTarget clamps to zero, never NaN", () => {
  const st = followWheel(followSuspend(followInit(), 10), 100, -50);
  assert.equal(st.manualTarget, 0);
  assert.ok(Number.isFinite(st.manualTarget));
});

test("resume re-engages auto target", () => {
  let st = followSuspend(followInit(), 100);
  assert.equal(effectiveTarget(st, 700), 100, "manual while suspended");
  st = followResume(st);
  assert.equal(effectiveTarget(st, 700), 700, "auto after resume");
});

test("resume is idempotent", () => {
  const st = followResume(followResume(followInit()));
  assert.equal(st.suspended, false);
});
