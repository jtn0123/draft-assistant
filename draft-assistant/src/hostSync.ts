// What a follower knows about the host it joined, taken off the host's own
// heartbeat rather than guessed at.
//
// A follower has no poller: `apiRemote`'s `startPolling` is a no-op that
// resolves, so the shell set its "polling" flag true whatever the host was
// doing, and the header showed a green Live pill over a board whose host had
// switched live sync off. The host's name had the same shape of problem from
// the other end: it was taken once, in the answer to `POST /api/pair`, so
// renaming the Mac under "Your name in shared chat" left every follower
// calling it by the name it had when they joined.
//
// Both facts ride on the `hello` frame the socket opens with and on every
// `pong` that answers a heartbeat, so nothing here is ever more than one
// heartbeat behind. It is module state for the same reason `followStatus` is:
// `api` picks its backend once, at import time, outside any component.

import { useSyncExternalStore } from "react";
import { saveFollow } from "./companion";
import type { FollowRecord } from "./types";

/** The host's own account of itself. `polling` is null until the host has
 *  said, which is not the same as "off" and must not be shown as either. */
export interface HostSync {
  polling: boolean | null;
  hostName: string | null;
}

const NOTHING_YET: HostSync = { polling: null, hostName: null };

let current: HostSync = NOTHING_YET;
const watchers = new Set<() => void>();

/**
 * Take what the host said about itself.
 *
 * The snapshot is replaced only when something in it actually changed:
 * `useSyncExternalStore` compares by identity, and a fresh object every
 * heartbeat would re-render the whole shell every twenty five seconds.
 */
export function setHostSync(next: HostSync): void {
  if (current.polling === next.polling && current.hostName === next.hostName) return;
  current = next;
  for (const notify of [...watchers]) notify();
}

export function getHostSync(): HostSync {
  return current;
}

export function watchHostSync(notify: () => void): () => void {
  watchers.add(notify);
  return () => {
    watchers.delete(notify);
  };
}

/** One module is shared by every test in a file; each one starts knowing
 *  nothing, which is what a window that has just opened knows. */
export function resetHostSync(): void {
  current = NOTHING_YET;
  for (const notify of [...watchers]) notify();
}

export function useHostSync(): HostSync {
  return useSyncExternalStore(watchHostSync, getHostSync, getHostSync);
}

/**
 * Read a `hello` or `pong` payload, and keep the follow record's name current.
 *
 * `follow` is the record the running backend holds, so the new name reaches
 * every message built from it (which host refused a write, which host picks
 * the league) as well as the header, and it is written back so a reload does
 * not go back to the old one. A frame that says nothing about a field leaves
 * that field as it was.
 */
export function noteHostStatus(follow: FollowRecord, payload: unknown): void {
  const status = (payload ?? {}) as { host_name?: unknown; polling?: unknown };
  const polling = typeof status.polling === "boolean" ? status.polling : current.polling;
  const named =
    typeof status.host_name === "string" && status.host_name !== "" ? status.host_name : null;
  if (named !== null && named !== follow.host_name) {
    follow.host_name = named;
    saveFollow(follow);
  }
  setHostSync({ polling, hostName: named ?? current.hostName ?? follow.host_name });
}
