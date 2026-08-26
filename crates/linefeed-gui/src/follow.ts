// Manual-scroll suspend state machine: a wheel gesture pauses auto-follow
// at a manual target; `f` (or the resume button) re-engages. Pure.

export interface FollowState {
  suspended: boolean;
  manualTarget: number;
}

export function followInit(): FollowState {
  return { suspended: false, manualTarget: 0 };
}

/** A wheel gesture while following suspends at the given current position. */
export function followSuspend(_st: FollowState, currentPos: number): FollowState {
  return { suspended: true, manualTarget: Math.max(0, currentPos) };
}

/** Accumulate wheel movement, clamped to the scrollable range. A negative
 * maxTarget (zone taller than content) clamps to 0, never NaN. */
export function followWheel(
  st: FollowState,
  deltaY: number,
  maxTarget: number,
): FollowState {
  const max = Math.max(0, maxTarget);
  const target = Math.max(0, Math.min(st.manualTarget + deltaY, max));
  return { suspended: true, manualTarget: target };
}

export function followResume(st: FollowState): FollowState {
  return { ...st, suspended: false };
}

/** The spring target: manual while suspended, auto otherwise. */
export function effectiveTarget(st: FollowState, autoTarget: number): number {
  return st.suspended ? st.manualTarget : autoTarget;
}
