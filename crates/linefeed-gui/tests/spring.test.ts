import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_OMEGA, settle, settled, springStep, type SpringState } from "../src/spring.ts";

function run(s: SpringState, target: number, steps: number, dt = 1 / 60): SpringState {
  for (let i = 0; i < steps; i++) s = springStep(s, target, dt);
  return s;
}

test("converges to the target", () => {
  const s = run({ pos: 0, vel: 0 }, 100, 600);
  assert.ok(Math.abs(s.pos - 100) < 0.1, `pos ${s.pos}`);
  assert.ok(Math.abs(s.vel) < 0.1);
});

test("never overshoots from rest (critical damping)", () => {
  let s: SpringState = { pos: 0, vel: 0 };
  for (let i = 0; i < 600; i++) {
    s = springStep(s, 100, 1 / 60);
    assert.ok(s.pos <= 100 + 1e-6, `overshoot at step ${i}: ${s.pos}`);
  }
});

test("approach is monotone from rest", () => {
  let s: SpringState = { pos: 0, vel: 0 };
  let prev = 0;
  for (let i = 0; i < 300; i++) {
    s = springStep(s, 50, 1 / 60);
    assert.ok(s.pos >= prev - 1e-9, `non-monotone at ${i}`);
    prev = s.pos;
  }
});

test("velocity rises then decays", () => {
  let s: SpringState = { pos: 0, vel: 0 };
  let peak = 0;
  let peakAt = 0;
  for (let i = 0; i < 300; i++) {
    s = springStep(s, 100, 1 / 60);
    if (s.vel > peak) {
      peak = s.vel;
      peakAt = i;
    }
  }
  assert.ok(peak > 0);
  assert.ok(peakAt > 0 && peakAt < 100, `velocity peak at ${peakAt}`);
  assert.ok(s.vel < peak / 10, "decayed at the end");
});

test("settle time is around 10.5 tau", () => {
  const tau = 1 / DEFAULT_OMEGA;
  let s: SpringState = { pos: 0, vel: 0 };
  let steps = 0;
  while (!settled(s, 1000, 0.5) && steps < 10000) {
    s = springStep(s, 1000, 1 / 60);
    steps++;
  }
  const t = steps / 60;
  assert.ok(t > 5 * tau && t < 15 * tau, `settled in ${t}s (tau=${tau})`);
});

test("mid-flight retarget is stable", () => {
  let s = run({ pos: 0, vel: 0 }, 100, 60);
  s = run(s, 20, 600);
  assert.ok(Math.abs(s.pos - 20) < 0.1);
});

test("exact at dt independence (closed form)", () => {
  const a = run({ pos: 0, vel: 0 }, 100, 60, 1 / 60);
  const b = run({ pos: 0, vel: 0 }, 100, 30, 2 / 60);
  assert.ok(Math.abs(a.pos - b.pos) < 1e-3, `${a.pos} vs ${b.pos}`);
});

test("settle() helper and purity", () => {
  const s = settle(42);
  assert.deepEqual(s, { pos: 42, vel: 0 });
  const before: SpringState = { pos: 1, vel: 2 };
  springStep(before, 10, 0.1);
  assert.deepEqual(before, { pos: 1, vel: 2 }, "input not mutated");
});
