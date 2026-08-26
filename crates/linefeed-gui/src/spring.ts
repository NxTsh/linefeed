// Critically-damped spring with EXACT closed-form integration:
//   u(t) = (A + B·t)·e^(−ω·t)
// No overshoot by construction (ζ = 1), stable at any dt.

export interface SpringState {
  pos: number;
  vel: number;
}

/** Natural frequency. τ ≈ 1/ω ≈ 357 ms — tuned for the continuous
 * intra-line glide (a stiffer spring visibly steps per word). */
export const DEFAULT_OMEGA = 2.8;

/** Advance the spring toward `target` by `dt` seconds. */
export function springStep(
  s: SpringState,
  target: number,
  dt: number,
  omega: number = DEFAULT_OMEGA,
): SpringState {
  const x = s.pos - target;
  const a = x;
  const b = s.vel + omega * x;
  const e = Math.exp(-omega * dt);
  const pos = target + (a + b * dt) * e;
  const vel = (b - omega * (a + b * dt)) * e;
  return { pos, vel };
}

/** Converged close enough to stop the rAF loop. */
export function settled(s: SpringState, target: number, eps = 0.5): boolean {
  return Math.abs(s.pos - target) < eps && Math.abs(s.vel) < eps;
}

export function settle(target: number): SpringState {
  return { pos: target, vel: 0 };
}
