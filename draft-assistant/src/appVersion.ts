// The running app's version, for the line at the bottom of the settings menu.

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

/** What the browser preview shows: it has no Tauri shell to ask, so this is
 *  kept in step with package.json and tauri.conf.json by hand. */
export const PREVIEW_VERSION = "0.2.0";

/** The running app's version, from the shell that knows it. */
export function useAppVersion(): string {
  const [version, setVersion] = useState(PREVIEW_VERSION);
  useEffect(() => {
    let cancelled = false;
    // Wrapped rather than called straight: outside Tauri this throws as it is
    // called, not as it settles, and the preview must simply keep the fallback
    // rather than take an unhandled rejection.
    void (async () => {
      try {
        const running = await getVersion();
        if (!cancelled) setVersion(running);
      } catch {
        // Not in the shell; PREVIEW_VERSION stands.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  return version;
}
