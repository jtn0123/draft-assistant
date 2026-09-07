// The running app's version, for the line at the bottom of the settings menu.

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { version as packageVersion } from "../package.json";

/** What the browser preview shows: it has no Tauri shell to ask, so it reads
 *  package.json at build time. It was a string kept in step by hand, and it
 *  sat at 0.2.0 for two releases; scripts/check-version.mjs holds package.json
 *  to the other two version files, so this cannot drift from any of them. */
export const PREVIEW_VERSION: string = packageVersion;

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
