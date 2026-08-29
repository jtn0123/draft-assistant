import { useCallback, useState } from "react";

/** Which side of the app is on screen: the draft cockpit, or the season. */
export type ViewMode = "draft" | "season";

const KEY = "draft-assistant.view-mode";

/** What a fresh eye wants: the draft until it is over, the season after. */
export function defaultViewMode(status: string | null | undefined): ViewMode {
  return status === "complete" ? "season" : "draft";
}

function storageKey(draftId: string): string {
  return `${KEY}:${draftId}`;
}

/**
 * A choice the user made for this draft, if any. Remembered per draft so
 * that switching to a mock draft mid-season does not drag the season screen
 * along with it, and coming back to the real league lands where it was left.
 */
export function loadViewMode(draftId: string): ViewMode | null {
  try {
    const v = window.localStorage.getItem(storageKey(draftId));
    return v === "draft" || v === "season" ? v : null;
  } catch {
    return null;
  }
}

export function saveViewMode(draftId: string, mode: ViewMode): void {
  try {
    window.localStorage.setItem(storageKey(draftId), mode);
  } catch {
    // Private mode or a full store: the switch still works for this session.
  }
}

type Saved = { id: string | null; mode: ViewMode | null };

function read(draftId: string | null): Saved {
  return { id: draftId, mode: draftId ? loadViewMode(draftId) : null };
}

/** The mode to show, and a setter that remembers the choice for this draft. */
export function useViewMode(
  draftId: string | null,
  status: string | null,
): [ViewMode, (mode: ViewMode) => void] {
  const [saved, setSaved] = useState<Saved>(() => read(draftId));
  // A different draft loads its own remembered choice (state derived from
  // props, reset during render rather than in an effect).
  if (saved.id !== draftId) setSaved(read(draftId));
  const set = useCallback(
    (mode: ViewMode) => {
      setSaved({ id: draftId, mode });
      if (draftId) saveViewMode(draftId, mode);
    },
    [draftId],
  );
  return [saved.mode ?? defaultViewMode(status), set];
}
