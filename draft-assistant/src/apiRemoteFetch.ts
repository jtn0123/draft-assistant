// The follower's one HTTP read, split out of apiRemote.ts as that file closed
// on the 500-line cap. Every `/api/*` GET a follower makes goes through here,
// so this is where the host's failure modes are turned into errors the shell
// can show.

import type { FollowRecord } from "./types";

/** How long one read of the host may take before it counts as unanswered.
 *  A host that accepts the connection and then says nothing (asleep, mid
 *  update, wedged) left the follower on the launch screen with no controls
 *  at all, because nothing ever rejected. Long enough for a slow Wi-Fi, short
 *  enough that the Try again and Leave host buttons appear while someone is
 *  still looking at the screen. */
export const HOST_TIMEOUT_MS = 10_000;

/** A GET that knows about the host's two failure modes: a 404 for something
 *  not loaded yet, and a 401 for a device that is no longer paired. A third,
 *  no answer at all, becomes an error after `HOST_TIMEOUT_MS`. */
export function remoteFetcher(follow: FollowRecord, onRevoked: () => void) {
  return async function fetchJson<T>(path: string): Promise<T | null> {
    // A controller and timer of our own rather than `AbortSignal.timeout`, so
    // the wait runs on the same clock as everything else in this window.
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), HOST_TIMEOUT_MS);
    let response: Response | null = null;
    try {
      response = await fetch(`${follow.url}${path}`, {
        headers: { authorization: `Bearer ${follow.token}` },
        signal: controller.signal,
      });
    } catch (e) {
      // Only our own abort is the timeout; anything else is the network's
      // and is rethrown as it came.
      if (!controller.signal.aborted) throw e;
    } finally {
      clearTimeout(timer);
    }
    if (response === null) {
      throw new Error(
        `${follow.host_name} did not answer within ${HOST_TIMEOUT_MS / 1000} seconds`,
      );
    }
    if (response.status === 401) {
      onRevoked();
      throw new Error("The host revoked this device");
    }
    if (response.status === 404) return null;
    if (!response.ok) throw new Error(`${follow.host_name} answered ${response.status}`);
    return (await response.json()) as T;
  };
}
