import { test } from "node:test";
import assert from "node:assert/strict";
import {
  clampZone,
  zoneBox,
  zoneStep,
  ZONE_HEIGHT_RANGE,
  ZONE_STEP_PCT,
  ZONE_WIDTH_RANGE,
} from "../src/zone.ts";

test("clamps mirror the Rust sanitize ranges", () => {
  assert.deepEqual(ZONE_WIDTH_RANGE, [40, 100]);
  assert.deepEqual(ZONE_HEIGHT_RANGE, [30, 100]);
  assert.equal(ZONE_STEP_PCT, 5);
  assert.deepEqual(clampZone(10, 10), { width: 40, height: 30 });
  assert.deepEqual(clampZone(150, 150), { width: 100, height: 100 });
  assert.deepEqual(clampZone(90, 80), { width: 90, height: 80 });
});

test("stepping clamps at the edges", () => {
  assert.equal(zoneStep(40, -5, ZONE_WIDTH_RANGE), 40);
  assert.equal(zoneStep(100, 5, ZONE_WIDTH_RANGE), 100);
  assert.equal(zoneStep(90, -5, ZONE_WIDTH_RANGE), 85);
});

test("zoneBox is centered", () => {
  const b = zoneBox(1000, 800, 90, 80);
  assert.equal(b.w, 900);
  assert.equal(b.h, 640);
  assert.equal(b.x, 50);
  assert.equal(b.y, 80);
  const full = zoneBox(1000, 800, 100, 100);
  assert.deepEqual(full, { x: 0, y: 0, w: 1000, h: 800 });
});
