// The Settings row "Import projections CSV…", minus the row itself.
//
// The picker, the parse, the copy into the app data dir and the re-match all
// happen in the backend — the app chooses the file, not the page. All this
// side owns is what the user is told afterwards.

import { api } from "./api";
import type { DraftView, SecondOpinionImport } from "./types";
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
 * What the toast says. The match counts come from the backend as a finished
 * sentence; the rows it refused to rank are appended here, because a user who
 * imported 482 rows and got 422 deserves to be told where the other sixty
 * went rather than left to wonder what the app lost.
 */
export function importToast(result: SecondOpinionImport): string {
  if (result.excluded_rows === 0 || result.excluded_reason === null) return result.message;
  const rows = result.excluded_rows === 1 ? "row" : "rows";
  return `${result.message}; ${result.excluded_rows} ${rows} skipped (${result.excluded_reason})`;
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
    showToast(importToast(result));
  } catch (e) {
    showToast(
      problem("Could not import those projections", e),
      () => void importSecondOpinion(applyView, showToast),
    );
  }
}
