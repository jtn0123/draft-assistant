import type { Page, Route } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { extname } from "node:path";
import { fileURLToPath } from "node:url";
import { dump } from "./fixtures";

/**
 * The fake host behind `companion.spec.ts`: the static files served out of
 * `src-tauri/companion-static/` under the same Content-Security-Policy the
 * Rust host sets, an `/api/*` that answers like the host, and a WebSocket
 * the test drives by hand.
 */

// The policy `companion/routes.rs` sets, read out of that file rather than
// copied here: the copy drifted from the source once already, and a page that
// passes under a looser policy than the phone gets proves nothing. The one
// substitution is the socket origins the host fills in per run; they are
// spelled out because a browser reads `connect-src 'self'` as the page's own
// scheme, and `ws://` is not `http://`.
const routesSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/companion/routes.rs", import.meta.url)),
  "utf8",
);
const cspTemplate = /format!\(\s*((?:"[^"]*"\s*)+)\)/.exec(
  routesSource.slice(routesSource.indexOf("pub fn csp_for")),
)?.[1];
if (cspTemplate === undefined) throw new Error("csp_for's format! string was not found");
const CSP = cspTemplate
  // Rust's `\` line continuation and the quotes around each piece.
  .replace(/\\\s*/g, "")
  .replace(/"/g, "")
  .replace("{connect}", "'self' ws://127.0.0.1:7878 ws://localhost:7878")
  // A header value with the macro's trailing newline in it is no header at
  // all: Chromium drops the whole response.
  .trim();
/** Content types by extension, as `routes.rs` serves them. */
const CONTENT_TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".css": "text/css",
  ".html": "text/html",
  ".webmanifest": "application/manifest+json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};
const staticDir = new URL("../src-tauri/companion-static/", import.meta.url);
/** Every file under `companion-static/`, as the host serves it: whatever is
 *  added there is served here without another list to keep in step. */
const STATIC_FILES = new Set(readdirSync(fileURLToPath(staticDir)));
const asset = (name: string): Buffer => readFileSync(fileURLToPath(new URL(name, staticDir)));
const contentType = (name: string): string =>
  CONTENT_TYPES[extname(name)] ?? "application/octet-stream";

export interface Backend {
  code: string;
  draft: Record<string, unknown> | null;
  season: Record<string, unknown> | null;
  chat: Record<string, unknown>;
  /** What the next `POST /api/chat` answers with. */
  postStatus: number;
  /** Flipped to false to make the host forget this device. */
  authorised: boolean;
}

export function backend(overrides: Partial<Backend> = {}): Backend {
  return {
    code: "424242",
    draft: dump("dev-fixture.json"),
    season: null,
    chat: {},
    postStatus: 202,
    authorised: true,
    ...overrides,
  };
}

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

/** Serve the three files and a host that answers the companion API. */
export async function serve(page: Page, host: Backend): Promise<void> {
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (path === "/") {
      return route.fulfill({
        body: asset("index.html"),
        headers: { "content-type": "text/html", "content-security-policy": CSP },
      });
    }
    // Every file under companion-static/. A file missing here is a 404 the
    // page never sees in production, and app.js dies on load when pwa.js is
    // one.
    const name = path.startsWith("/static/") ? path.slice("/static/".length) : "";
    if (name !== "" && STATIC_FILES.has(name)) {
      return route.fulfill({ body: asset(name), headers: { "content-type": contentType(name) } });
    }
    if (path === "/api/pair") {
      const sent = route.request().postDataJSON() as { code: string; device_name: string };
      return sent.code === host.code
        ? json(route, 200, { token: "tok-1", host_name: "Justin's Mac", device_id: "dev-1" })
        : json(route, 403, { error: "wrong code" });
    }
    if (!host.authorised) return json(route, 401, { error: "not paired" });
    if (path === "/api/state") {
      return host.draft ? json(route, 200, host.draft) : json(route, 404, { error: "no league" });
    }
    if (path === "/api/season") {
      return host.season ? json(route, 200, host.season) : json(route, 404, { error: "no league" });
    }
    if (path === "/api/chat") {
      if (route.request().method() === "POST") {
        return json(route, host.postStatus, host.postStatus === 202 ? { entry_id: "e9" } : {});
      }
      const screen = url.searchParams.get("screen") ?? "draft";
      const thread = host.chat[screen];
      return thread ? json(route, 200, thread) : json(route, 404, { error: "no thread" });
    }
    return route.fulfill({ status: 404, body: "" });
  });

  // A WebSocket the test drives by hand. The page only ever uses `onopen`,
  // `onmessage`, `onclose`, `send` and `readyState`.
  await page.addInitScript(() => {
    const store = window as unknown as {
      __sent: string[];
      __emit: (frame: unknown) => void;
      __socketUrl: string;
      __stayDown: boolean;
      __drop: (code?: number) => void;
    };
    store.__sent = [];
    store.__stayDown = false;
    class FakeSocket {
      readyState = 0;
      onopen: (() => void) | null = null;
      onclose: ((event: { code: number }) => void) | null = null;
      onmessage: ((event: { data: string }) => void) | null = null;
      constructor(url: string) {
        store.__socketUrl = url;
        store.__emit = (frame) => this.onmessage?.({ data: JSON.stringify(frame) });
        store.__drop = (code?: number) => this.close(code);
        if (store.__stayDown) return;
        setTimeout(() => {
          this.readyState = 1;
          this.onopen?.();
        }, 0);
      }
      send(data: string) {
        store.__sent.push(data);
      }
      close(code = 1006) {
        this.readyState = 3;
        this.onclose?.({ code });
      }
    }
    (window as unknown as { WebSocket: unknown }).WebSocket = FakeSocket;
  });
}

/** Push one server frame down the fake socket, once the page has opened it. */
export async function emit(page: Page, frame: unknown): Promise<void> {
  await page.waitForFunction(
    () => typeof (window as unknown as { __emit?: unknown }).__emit === "function",
  );
  await page.evaluate(
    (sent) => (window as unknown as { __emit: (f: unknown) => void }).__emit(sent),
    frame,
  );
}

/** Enter the code and land on Now. */
export async function pair(page: Page, code = "424242"): Promise<void> {
  await page.goto("/");
  await page.getByLabel("Pairing code").fill(code);
  await page.getByRole("button", { name: "Connect" }).click();
}

export function thread(screen: string, entries: unknown[], busy = false) {
  return { league_id: "L1", screen, busy, entries };
}

export const ask = (text: string, name = "Rob's iPhone") => ({
  id: "e1",
  at_ms: Date.now() - 120_000,
  device: { name, kind: "phone" },
  role: "user",
  text,
  cost_usd: null,
  error: null,
});

export const answer = (text: string, name = "Rob's iPhone") => ({
  id: "e2",
  at_ms: Date.now() - 60_000,
  device: { name, kind: "phone" },
  role: "assistant",
  text,
  cost_usd: 0.0184,
  error: null,
});
