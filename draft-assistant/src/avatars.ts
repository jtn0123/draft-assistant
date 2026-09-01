// Player pictures: the Headshots / Team logos preference, and a per-session
// cache in front of the backend's on-disk one so each face is resolved once.

import { useSyncExternalStore } from "react";
import { api } from "./api";

export type AvatarMode = "headshots" | "logos";

const STORAGE_KEY = "da.avatars";
const listeners = new Set<() => void>();
let mode: AvatarMode = read();

function read(): AvatarMode {
  try {
    return localStorage.getItem(STORAGE_KEY) === "logos" ? "logos" : "headshots";
  } catch {
    return "headshots";
  }
}

/**
 * The current mode, read without subscribing. Components use `useAvatarMode`;
 * this backs it as `useSyncExternalStore`'s snapshot, and is exported so a
 * test can read the mode outside a render.
 */
export function avatarMode(): AvatarMode {
  return mode;
}

export function setAvatarMode(next: AvatarMode): void {
  mode = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Not remembered in a sandboxed webview; still applies for this session.
  }
  for (const l of listeners) l();
}

export function useAvatarMode(): AvatarMode {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    avatarMode,
    avatarMode,
  );
}

/**
 * Cache `request` under `key`, but only if it produces an answer.
 *
 * "No picture" comes back two different ways and they must not be conflated.
 * A *resolved* null is an answer — Sleeper genuinely has no photo for this
 * player — and is worth remembering for the session so every render does not
 * re-ask. A *rejection* is not an answer: the request failed (offline, a
 * transport error, the backend's `headshot fetch: …`), and remembering it
 * would blank that one face until the app restarts, which is exactly the bug
 * where a single network blip left a player's picture permanently empty.
 *
 * So on rejection the entry is dropped again and the caller is told "nothing
 * for now" — the component falls back to the team logo, and the *next* time
 * something asks for this id (a remount, a tab switch, a refresh) the request
 * is made afresh. Deliberately no retry timer or automatic re-render: a
 * failure should simply not be remembered as an answer, and turning one blip
 * into a background retry loop across every visible face is worse than a
 * logo that becomes a photo again the next time you come back to the tab.
 */
function remember(
  cache: Map<string, Promise<string | null>>,
  key: string,
  request: Promise<string | null>,
): Promise<string | null> {
  const pending: Promise<string | null> = request.catch(() => {
    // Only forget our own entry: a reset (or a later ask that already
    // replaced it) must not have its answer torn out by this stale failure.
    if (cache.get(key) === pending) cache.delete(key);
    return null;
  });
  cache.set(key, pending);
  return pending;
}

const resolved = new Map<string, Promise<string | null>>();

/** The image source for a player, or null when Sleeper has no photo. Each id
 * is asked of the backend once per session; the backend keeps it on disk. A
 * request that *fails* is not an answer and is not remembered — see
 * {@link remember}. */
export function headshotSrc(playerId: string): Promise<string | null> {
  const pending = resolved.get(playerId);
  if (pending !== undefined) return pending;
  return remember(resolved, playerId, api.headshot(playerId));
}

const teams = new Map<string, Promise<string | null>>();

/** The image source for a manager's team picture. Same one-ask-per-session
 * rule as the player photos; the backend keeps it on disk. `full` asks for
 * the 280px copy Sleeper serves for the zoomed view. */
export function teamAvatarSrc(reference: string, full = false): Promise<string | null> {
  const key = full ? `full:${reference}` : reference;
  const pending = teams.get(key);
  if (pending !== undefined) return pending;
  return remember(teams, key, api.avatar(reference, full));
}

/** Test hook: forget everything resolved so far. */
export function resetAvatarCache(): void {
  resolved.clear();
  teams.clear();
}
