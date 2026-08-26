import { test } from "node:test";
import assert from "node:assert/strict";
import {
  advancePhase,
  dismissAt,
  shouldDismiss,
  MIN_DWELL_MS,
  PHASE_COPY,
} from "../src/splash.ts";

test("phases advance forward only (stale events ignored)", () => {
  let p = advancePhase("booting", "engine-ready");
  assert.equal(p, "engine-ready");
  p = advancePhase(p, "script-loaded");
  assert.equal(p, "script-loaded");
  p = advancePhase(p, "booting");
  assert.equal(p, "script-loaded", "never moves backwards");
});

test("dismiss requires done + no fetch hold + no error", () => {
  assert.equal(shouldDismiss({ phase: "done", fetchHold: false, error: false }), true);
  assert.equal(shouldDismiss({ phase: "booting", fetchHold: false, error: false }), false);
  assert.equal(shouldDismiss({ phase: "done", fetchHold: true, error: false }), false);
  assert.equal(shouldDismiss({ phase: "done", fetchHold: false, error: true }), false);
});

test("dwell math: fast startup still holds the full beat", () => {
  assert.equal(dismissAt(0, 200), MIN_DWELL_MS, "200ms boot → 3s dwell");
  assert.equal(dismissAt(0, 4000), 4000, "slow boot is not truncated");
  assert.equal(dismissAt(0, 200, 0), 200, "dwell 0 disables the beat");
  assert.equal(dismissAt(1000, 1200), 1000 + MIN_DWELL_MS, "offset independent");
});

test("every phase has copy (done is empty by design)", () => {
  assert.ok(PHASE_COPY["booting"].length > 0);
  assert.ok(PHASE_COPY["engine-ready"].length > 0);
  assert.equal(PHASE_COPY["done"], "");
});
