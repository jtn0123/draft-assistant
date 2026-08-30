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

const resolved = new Map<string, Promise<string | null>>();

/** The image source for a player, or null when Sleeper has no photo. Each id
 * is asked of the backend once per session; the backend keeps it on disk. */
export function headshotSrc(playerId: string): Promise<string | null> {
  let pending = resolved.get(playerId);
  if (pending === undefined) {
    pending = api.headshot(playerId).catch(() => null);
    resolved.set(playerId, pending);
  }
  return pending;
}

const teams = new Map<string, Promise<string | null>>();

/** The image source for a manager's team picture. Same one-ask-per-session
 * rule as the player photos; the backend keeps it on disk. `full` asks for
 * the 280px copy Sleeper serves for the zoomed view. */
export function teamAvatarSrc(reference: string, full = false): Promise<string | null> {
  const key = full ? `full:${reference}` : reference;
  let pending = teams.get(key);
  if (pending === undefined) {
    pending = api.avatar(reference, full).catch(() => null);
    teams.set(key, pending);
  }
  return pending;
}

/** Test hook: forget everything resolved so far. */
export function resetAvatarCache(): void {
  resolved.clear();
  teams.clear();
}
