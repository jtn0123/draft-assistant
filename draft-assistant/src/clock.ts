// The wall clock, as one module store rather than a timer per component.
//
// Grade item G7. The draft banner and the pick queue both count the pick clock
// down, and each used to own a `setInterval` and a piece of `now` state. That
// meant two unsynchronised re-renders of the whole draft subtree every second
// — on top of the 3-second poll — and two timers that could land either side
// of a second boundary and print different values for the same tick.
//
// One interval for the whole app, one `now`, every consumer reading it through
// `useSyncExternalStore` like the other small stores here (avatars, prefs,
// zoom). The interval only runs while something is actually watching it.

import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();
let timer: number | null = null;
// Seeded at import so the very first render, which happens before React has
// called `subscribe`, has a real reading rather than 1970.
let now = Date.now();

function tick(): void {
  now = Date.now();
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (timer === null) {
    now = Date.now();
    timer = window.setInterval(tick, 1000);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  };
}

/**
 * No timer wanted: the value is refreshed as this consumer subscribes and
 * never notified about again.
 *
 * The refresh happens here rather than in `snapshot` because `snapshot` is
 * `getSnapshot`, and React calls that while it is deciding whether anything
 * changed — during render, and twice in a row in StrictMode. A getter that
 * writes to the store it is reading can hand back two different values for one
 * render pass, which is exactly the "getSnapshot should be cached" loop React
 * warns about. Subscribing is an effect, so it is a safe place to write.
 */
function ignore(): () => void {
  if (timer === null) now = Date.now();
  return () => {
    // Nothing subscribed, so nothing to tear down.
  };
}

function snapshot(): number {
  return now;
}

/**
 * The current time in milliseconds, re-rendering once a second while `active`.
 *
 * Inactive callers still get a usable reading; they simply are not woken for
 * each tick, which is what a component with nothing on the clock wants.
 */
export function useNow(active: boolean): number {
  return useSyncExternalStore(active ? subscribe : ignore, snapshot, snapshot);
}

/**
 * Below this, a difference between the host's clock and this one is view age
 * and second-rounding rather than a wrong clock: the view is stamped in whole
 * seconds and is up to a poll interval old by the time it is read.
 */
const SKEW_TOLERANCE_MS = 30_000;

/** Past this the user is told, rather than being left to wonder. */
const SKEW_NOTICE_MS = 60_000;

/**
 * How far this device's clock is from the clock the pick deadline was built
 * on, in milliseconds: positive when this device is *behind*.
 *
 * The pick deadline comes from the platform's own stamps, and it used to be
 * counted down against `Date.now()` with nothing in between. A laptop two
 * minutes fast therefore showed 0:00 while the drafter still had ninety
 * seconds, and a phone on the companion showed a different number from the
 * machine beside it. `generated_at` is when the host built the view, so the
 * gap between it and now is that host's clock measured against this one.
 *
 * Small differences are read as zero on purpose: the correction is there for
 * a clock that is wrong, not for the two or three seconds of poll latency
 * every view carries.
 */
export function clockOffsetMs(generatedAtSecs: number, nowMs: number): number {
  if (!Number.isFinite(generatedAtSecs) || generatedAtSecs <= 0) return 0;
  const offset = generatedAtSecs * 1000 - nowMs;
  return Math.abs(offset) < SKEW_TOLERANCE_MS ? 0 : offset;
}

/**
 * What to say on the banner when the correction is big enough to matter, or
 * null when it is not.
 *
 * A countdown quietly running minutes out is worse than one that admits it,
 * because the whole point of the number is deciding whether there is time.
 */
export function clockSkewNote(offsetMs: number): string | null {
  if (Math.abs(offsetMs) < SKEW_NOTICE_MS) return null;
  const minutes = Math.max(1, Math.round(Math.abs(offsetMs) / 60_000));
  const unit = minutes === 1 ? "minute" : "minutes";
  const direction = offsetMs > 0 ? "behind" : "ahead of";
  return `clock corrected: this device is ${minutes} ${unit} ${direction} the draft host`;
}

/** Test seam: how many consumers the single interval is currently feeding. */
export function clockListenerCount(): number {
  return listeners.size;
}
