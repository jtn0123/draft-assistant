// Small preferences that several components read but nothing owns: kept in a
// module store, like the avatar mode, so they do not have to be threaded
// through App.tsx and down as props.

import { useSyncExternalStore } from "react";

const CHIME_KEY = "da.chime";
const listeners = new Set<() => void>();

function readChime(): boolean {
  try {
    return localStorage.getItem(CHIME_KEY) !== "off";
  } catch {
    return true;
  }
}

let chime = readChime();

export function chimeOn(): boolean {
  return chime;
}

export function setChime(next: boolean): void {
  chime = next;
  try {
    localStorage.setItem(CHIME_KEY, next ? "on" : "off");
  } catch {
    // Not remembered in a sandboxed webview; still applies for this session.
  }
  for (const l of listeners) l();
}

export function useChime(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    chimeOn,
    chimeOn,
  );
}

/** Test seam: reset the store between cases. */
export function resetPrefs(): void {
  chime = true;
  for (const l of listeners) l();
}
