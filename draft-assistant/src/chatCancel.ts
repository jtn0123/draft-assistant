// The Cancel button's one call.
//
// Deliberately not on `api`: that surface is mirrored by the follower's
// remote backend and the browser preview, and neither has an answer in
// flight to stop. Ask Claude only ever runs against the desktop backend, so
// this goes straight to it, and does nothing at all anywhere else.

import { invoke } from "@tauri-apps/api/core";

/** Stop the answer this screen is waiting on. Resolves to whether there was
 *  one to stop; the stopped answer comes back through `askClaude`, marked
 *  cut short. Never rejects: a cancel that failed leaves the answer running,
 *  and the panel is still waiting on it either way. */
export async function cancelClaude(screen: string): Promise<boolean> {
  if (!("__TAURI_INTERNALS__" in window)) return false;
  try {
    return await invoke<boolean>("cancel_claude", { screen });
  } catch {
    return false;
  }
}
