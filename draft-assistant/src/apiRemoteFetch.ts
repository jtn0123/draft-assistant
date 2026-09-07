// The follower's HTTP layer, split out of apiRemote.ts as that file closed on
// the 500-line cap. Every request a follower makes to the host, reads and
// writes alike, goes through `hostFetch`, so this is the one place a host that
// says nothing is turned into an error the shell can show.

import type { FollowRecord } from "./types";

/** How long one request to the host may take before it counts as unanswered.
 *  A host that accepts the connection and then says nothing (asleep, mid
 *  update, wedged) left the follower on the launch screen with no controls
 *  at all, because nothing ever rejected. Long enough for a slow Wi-Fi, short
 *  enough that the Try again and Leave host buttons appear while someone is
 *  still looking at the screen. The same figure covers a question sent to the
 *  shared thread: the host answers a POST with 202 at once and does the
 *  thinking afterwards, so a send that takes longer than a read is not slow,
 *  it is lost. */
export const HOST_TIMEOUT_MS = 10_000;

/** The name on the error a request raises when the host never answered. */
export const HOST_TIMEOUT = "HostTimeout";

function timedOut(hostName: string, timeoutMs: number): Error {
  const error = new Error(`${hostName} did not answer within ${timeoutMs / 1000} seconds`);
  error.name = HOST_TIMEOUT;
  return error;
}

/**
 * `fetch` with a deadline.
 *
 * Resolves to the response, whatever its status; the caller reads the status
 * because a 404 means one thing on a read and another on a write. Rejects
 * with a `HOST_TIMEOUT` error after `timeoutMs` of silence, and with the
 * network's own error for anything else. A controller and timer of our own
 * rather than `AbortSignal.timeout`, so the wait runs on the same clock as
 * everything else in this window and the tests can wind it.
 *
 * `hostName` is only for the message; pairing uses this before there is a
 * host name to use and passes the address instead.
 */
export async function timedFetch(
  url: string,
  init: RequestInit,
  hostName: string,
  timeoutMs: number = HOST_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } catch (e) {
    // Only our own abort is the timeout; anything else is the network's
    // and is rethrown as it came.
    if (controller.signal.aborted) throw timedOut(hostName, timeoutMs);
    throw e;
  } finally {
    clearTimeout(timer);
  }
}

/** `timedFetch` against the host this window follows, with its pairing
 *  token. Every request a follower makes after pairing goes through here. */
export function hostFetch(
  follow: FollowRecord,
  path: string,
  init: RequestInit = {},
  timeoutMs: number = HOST_TIMEOUT_MS,
): Promise<Response> {
  return timedFetch(
    `${follow.url}${path}`,
    { ...init, headers: { authorization: `Bearer ${follow.token}`, ...(init.headers ?? {}) } },
    follow.host_name,
    timeoutMs,
  );
}

/** A GET that knows about the host's two failure modes: a 404 for something
 *  not loaded yet, and a 401 for a device that is no longer paired. A third,
 *  no answer at all, becomes an error after `HOST_TIMEOUT_MS`. */
export function remoteFetcher(follow: FollowRecord, onRevoked: () => void) {
  return async function fetchJson<T>(path: string): Promise<T | null> {
    const response = await hostFetch(follow, path);
    if (response.status === 401) {
      onRevoked();
      throw new Error("The host revoked this device");
    }
    if (response.status === 404) return null;
    if (!response.ok) throw new Error(`${follow.host_name} answered ${response.status}`);
    return (await response.json()) as T;
  };
}
