// Whether a follower can still hear the host it joined.
//
// The socket in `apiRemote` is the only thing that knows, and the header is
// the only thing that shows it, so the state sits in this small module
// between them. It is module state rather than a React context because `api`
// picks its backend once, at import time, outside any component; a follower
// that had gone quiet used to look exactly like one whose host had simply not
// picked yet, which is the failure this exists to prevent.

import { useSyncExternalStore } from "react";

export type FollowStatus = "connected" | "reconnecting" | "revoked";

/** The close code the host uses for a token it no longer knows. */
export const REVOKED_CLOSE_CODE = 4401;

let current: FollowStatus = "connected";
const watchers = new Set<() => void>();

function announce(): void {
  for (const notify of [...watchers]) notify();
}

/**
 * Move the follower to a new connection state.
 *
 * Being dropped is final: a socket closing a moment after the host revoked
 * this device must not walk the message back to "reconnecting", which would
 * promise a reconnection that can never happen.
 */
export function setFollowStatus(next: FollowStatus): void {
  if (current === "revoked" || current === next) return;
  current = next;
  announce();
}

export function getFollowStatus(): FollowStatus {
  return current;
}

export function watchFollowStatus(notify: () => void): () => void {
  watchers.add(notify);
  return () => {
    watchers.delete(notify);
  };
}

/** One module is shared by every test in a file; each one starts connected. */
export function resetFollowStatus(): void {
  current = "connected";
  announce();
}

export function useFollowStatus(): FollowStatus {
  return useSyncExternalStore(watchFollowStatus, getFollowStatus, getFollowStatus);
}

/**
 * The line beside the "Hosted by" pill, or null when there is nothing to say.
 *
 * Connected is the ordinary case and gets no words of its own: a header that
 * says "Connected" all draft long teaches people to stop reading it.
 */
export function followStatusMessage(status: FollowStatus, hostName: string): string | null {
  if (status === "reconnecting") return `Reconnecting to ${hostName}…`;
  if (status === "revoked") return `${hostName} unpaired this device`;
  return null;
}
