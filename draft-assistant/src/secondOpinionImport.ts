// The Settings row "Import projections CSV…", minus the row itself.
//
// The picker, the parse, the copy into the app data dir and the re-match all
// happen in the backend — the app chooses the file, not the page. All this
// side owns is what the user is told afterwards.

import { api } from "./api";
import type { DraftView } from "./types";
import { problem } from "./format";

/** How the note under the Settings row reads. */
export function importNote(loadedAt: number | null | undefined, source: string | null): string {
  if (loadedAt === null || loadedAt === undefined || loadedAt === 0) {
    return "Add a second opinion column to the board";
  }
  const when = new Date(loadedAt * 1000).toLocaleDateString();
  return `${source ?? "Imported"} loaded ${when} — import again to replace`;
}

/**
 * Run the import. Cancelling the picker is silent: the user changed their
 * mind, and a toast saying so would be noise.
 */
export async function importSecondOpinion(
  applyView: (view: DraftView) => void,
  showToast: (text: string, retry?: () => void) => void,
): Promise<void> {
  try {
    const result = await api.importSecondOpinion();
    if (result === null) return;
    applyView(result.view);
    showToast(result.message);
  } catch (e) {
    showToast(
      problem("Could not import those projections", e),
      () => void importSecondOpinion(applyView, showToast),
    );
  }
}
