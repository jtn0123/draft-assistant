// The picture you clicked, shown large. One at a time, so it lives in a tiny
// store rather than being threaded through every table that draws a face.

import { useSyncExternalStore } from "react";

export interface Zoomed {
  /** Already-resolved image source for the small version. */
  src: string;
  /** Who or what it is, shown under the picture. */
  label: string;
  /** An avatar reference, when a larger copy can be fetched for the zoom. */
  avatar?: string;
}

let current: Zoomed | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const l of listeners) l();
}

export function openZoom(next: Zoomed): void {
  current = next;
  emit();
}

export function closeZoom(): void {
  current = null;
  emit();
}

function snapshot(): Zoomed | null {
  return current;
}

export function useZoom(): Zoomed | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    snapshot,
    snapshot,
  );
}
