import { openUrl } from "@tauri-apps/plugin-opener";

const inTauri = "__TAURI_INTERNALS__" in window;

/**
 * Send a link to the real browser. Never to this webview: the app is a single
 * page with a live poll loop behind it, and navigating away mid-draft would
 * take the draft with it.
 */
export async function openExternal(url: string): Promise<void> {
  if (!isSafeUrl(url)) return;
  if (inTauri) {
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

/** http(s) only — a `javascript:` href in an answer is not a link. */
export function isSafeUrl(url: string): boolean {
  try {
    const scheme = new URL(url).protocol;
    return scheme === "http:" || scheme === "https:";
  } catch {
    return false;
  }
}
