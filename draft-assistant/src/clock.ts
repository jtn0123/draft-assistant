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
let now = 0;

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

/** No timer wanted: the value is read once and never notified about. */
function ignore(): () => void {
  return () => {
    // Nothing subscribed, so nothing to tear down.
  };
}

function snapshot(): number {
  // With the interval stopped nothing is keeping `now` fresh, so read the
  // system clock. Held for a second at a time so that repeated calls within
  // one render agree with each other — `useSyncExternalStore` needs a snapshot
  // that only changes when there is genuinely something new to show.
  if (timer === null) {
    const t = Date.now();
    if (t < now || t - now >= 1000) now = t;
  }
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

/** Test seam: how many consumers the single interval is currently feeding. */
export function clockListenerCount(): number {
  return listeners.size;
}
