// One implementation of "a small choice the app remembers".
//
// Storage throws in a private window and in some locked-down webviews, so
// every read and write is guarded: a preference we cannot save still applies
// for the rest of the session. Each preference is a module store, so any
// component can subscribe to it instead of having it threaded down as a prop.

import { useSyncExternalStore } from "react";

export interface Persisted<T extends string> {
  /** The value in force right now. */
  get: () => T;
  /** Change it, remember it, and tell everyone reading it. */
  set: (next: T) => void;
  /** Listen for changes; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
  /** Test seam: drop the session's choice and go back to what is stored. */
  reset: () => void;
}

/**
 * A remembered preference, stored as the exact text the app has always
 * written, so upgrading never loses somebody's choice.
 *
 * @param key the storage key, unchanged from whoever wrote it first
 * @param parse the stored text turned into a value, or null when it is not
 *   one of ours
 * @param fallback used when nothing is stored, the text is unrecognised, or
 *   storage refuses to answer at all
 */
export function persisted<T extends string>(
  key: string,
  parse: (raw: string) => T | null,
  fallback: T,
): Persisted<T> {
  const listeners = new Set<() => void>();
  // What the user picked this session. Kept separately from storage so the
  // choice still holds when the write was refused.
  let chosen: T | null = null;

  const stored = (): T => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? fallback : (parse(raw) ?? fallback);
    } catch {
      return fallback;
    }
  };

  const announce = () => {
    for (const listener of listeners) listener();
  };

  return {
    get: () => chosen ?? stored(),
    set: (next) => {
      chosen = next;
      try {
        localStorage.setItem(key, next);
      } catch {
        // Not remembered for next time; still applies for this session.
      }
      announce();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    reset: () => {
      chosen = null;
      announce();
    },
  };
}

/** Read a preference in a component, re-rendering when anything changes it. */
export function usePersisted<T extends string>(store: Persisted<T>): T {
  return useSyncExternalStore(store.subscribe, store.get, store.get);
}
