// Serving a replay source to the browser preview, from a test.
//
// `?replay=<url>` makes the preview re-read that URL every three seconds and
// push anything newer into the app (see `src/replay.ts`). A Playwright route
// is the cheapest possible stand-in for `scripts/replay-sleeper.mjs`: it hands
// out the checked-in dump, then whatever the test edits into it.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";

const publicDir = new URL("../public/", import.meta.url);

/** The checked-in dump, parsed fresh so a test can edit its own copy. */
export function dump(name: string): Record<string, unknown> {
  return JSON.parse(readFileSync(fileURLToPath(new URL(name, publicDir)), "utf8")) as Record<
    string,
    unknown
  >;
}

/** How a test rewrites the dump it is serving. */
export interface ReplayServer {
  /** Replace what the next poll will read. */
  write(next: Record<string, unknown>): void;
  /** What was served last, for editing and writing back. */
  latest(): Record<string, unknown>;
}

/**
 * Serve `path` from memory, the way the replay server serves a file it keeps
 * rewriting. Every request reads the current value, so a `write` between
 * polls is exactly the recording moving on.
 *
 * Matched on the pathname alone, and deliberately not with a glob: the page
 * itself is opened at `/?replay=<path>`, which any `**` pattern ending in the
 * file name would swallow — serving the dump as the document.
 */
export async function serveReplay(
  page: Page,
  path: string,
  initial: Record<string, unknown>,
): Promise<ReplayServer> {
  let current = initial;
  await page.route(
    (url) => url.pathname === path,
    (route) => route.fulfill({ contentType: "application/json", body: JSON.stringify(current) }),
  );
  return {
    write: (next) => {
      current = next;
    },
    latest: () => current,
  };
}
