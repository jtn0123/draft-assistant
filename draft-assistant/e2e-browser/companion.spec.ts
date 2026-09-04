import { expect, test, type Page, type Route } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dump } from "./fixtures";

/**
 * The phone companion page, driven by a real browser.
 *
 * The page is three static files that the Rust host serves at `/` and
 * `/static/*`; nothing builds them, so the test serves them itself out of
 * `src-tauri/companion-static/` with the same Content-Security-Policy the
 * host sets. That policy is the point of serving them rather than pasting
 * markup into the test: if anything inline ever creeps into the page, the
 * browser refuses to run it here exactly as it would on a phone.
 *
 * The host's WebSocket is replaced by a controllable fake, so a test can push
 * a `draft-updated` or `shared-chat` frame at the exact moment it wants one.
 */

const CSP = "default-src 'self'; connect-src 'self' ws:";
const staticDir = new URL("../src-tauri/companion-static/", import.meta.url);
const asset = (name: string) => readFileSync(fileURLToPath(new URL(name, staticDir)), "utf8");

interface Backend {
  code: string;
  draft: Record<string, unknown> | null;
  season: Record<string, unknown> | null;
  chat: Record<string, unknown>;
  /** What the next `POST /api/chat` answers with. */
  postStatus: number;
  /** Flipped to false to make the host forget this device. */
  authorised: boolean;
}

function backend(overrides: Partial<Backend> = {}): Backend {
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
async function serve(page: Page, host: Backend): Promise<void> {
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (path === "/") {
      return route.fulfill({
        body: asset("index.html"),
        headers: { "content-type": "text/html", "content-security-policy": CSP },
      });
    }
    if (["/static/helpers.js", "/static/app.js", "/static/app.css"].includes(path)) {
      const type = path.endsWith(".css") ? "text/css" : "text/javascript";
      const body = asset(path.slice("/static/".length));
      return route.fulfill({ body, headers: { "content-type": type } });
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
      __drop: () => void;
    };
    store.__sent = [];
    store.__stayDown = false;
    class FakeSocket {
      readyState = 0;
      onopen: (() => void) | null = null;
      onclose: (() => void) | null = null;
      onmessage: ((event: { data: string }) => void) | null = null;
      constructor(url: string) {
        store.__socketUrl = url;
        store.__emit = (frame) => this.onmessage?.({ data: JSON.stringify(frame) });
        store.__drop = () => this.close();
        if (store.__stayDown) return;
        setTimeout(() => {
          this.readyState = 1;
          this.onopen?.();
        }, 0);
      }
      send(data: string) {
        store.__sent.push(data);
      }
      close() {
        this.readyState = 3;
        this.onclose?.();
      }
    }
    (window as unknown as { WebSocket: unknown }).WebSocket = FakeSocket;
  });
}

/** Push one server frame down the fake socket, once the page has opened it. */
async function emit(page: Page, frame: unknown): Promise<void> {
  await page.waitForFunction(
    () => typeof (window as unknown as { __emit?: unknown }).__emit === "function",
  );
  await page.evaluate(
    (sent) => (window as unknown as { __emit: (f: unknown) => void }).__emit(sent),
    frame,
  );
}

/** Enter the code and land on Now. */
async function pair(page: Page, code = "424242"): Promise<void> {
  await page.goto("/");
  await page.getByLabel("Pairing code").fill(code);
  await page.getByRole("button", { name: "Connect" }).click();
}

function thread(screen: string, entries: unknown[], busy = false) {
  return { league_id: "L1", screen, busy, entries };
}

const ask = (text: string, name = "Rob's iPhone") => ({
  id: "e1",
  at_ms: Date.now() - 120_000,
  device: { name, kind: "phone" },
  role: "user",
  text,
  cost_usd: null,
  error: null,
});

const answer = (text: string, name = "Rob's iPhone") => ({
  id: "e2",
  at_ms: Date.now() - 60_000,
  device: { name, kind: "phone" },
  role: "assistant",
  text,
  cost_usd: 0.0184,
  error: null,
});

test("refuses the wrong code and opens the draft on the right one", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page, "000000");
  await expect(page.getByRole("alert")).toHaveText("That code did not work.");
  await expect(page.getByRole("button", { name: "Now" })).toBeHidden();

  await page.getByLabel("Pairing code").fill(host.code);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#clock-strip")).toContainText("Pick");
  await expect(page.getByRole("button", { name: "Now" })).toHaveAttribute("aria-current", "page");
  // The fixture has us on the clock, and the host's name is now known.
  await expect(page.locator("#clock-strip")).toContainText("Your pick");
  expect(await page.evaluate(() => window.localStorage.getItem("da.companion.token"))).toBe(
    "tok-1",
  );
});

test("names the device from the user agent and remembers what was typed", async ({ page }) => {
  await serve(page, backend());
  await page.goto("/");
  await expect(page.getByLabel("This device")).toHaveValue("Phone");
  await page.getByLabel("This device").fill("Rob's iPhone");
  await page.getByLabel("Pairing code").fill("424242");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.locator("#clock-strip")).toContainText("Pick");
  expect(await page.evaluate(() => window.localStorage.getItem("da.companion.device"))).toBe(
    "Rob's iPhone",
  );
});

test("renders the recommendations the fixture captured", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page);
  const fixture = host.draft as { recommendations: { name: string; reasons: string[] }[] };
  const top = fixture.recommendations[0];
  const card = page.locator("#recs .card").first();
  await expect(card.locator(".name")).toHaveText(top.name);
  await expect(card.locator(".pos")).toHaveCount(1);
  await expect(card.locator(".facts")).toContainText("Tier");
  await expect(card.locator(".facts")).toContainText("ADP");
  await expect(card.locator(".reasons li")).toHaveCount(top.reasons.length);
  await expect(card).toContainText(top.reasons[0]);
});

test("lists the most recent picks newest first", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Picks" }).click();
  const rows = page.locator("#picks .row");
  const count = await rows.count();
  expect(count).toBeGreaterThan(0);
  expect(count).toBeLessThanOrEqual(25);
  const fixture = host.draft as { recent_picks: { pick_no: number; name: string }[] };
  const newest = [...fixture.recent_picks].sort((a, b) => b.pick_no - a.pick_no)[0];
  await expect(rows.first()).toContainText(newest.name);
});

test("attributes chat entries, renders light markdown and shows the cost", async ({ page }) => {
  const host = backend({
    chat: {
      draft: thread("draft", [
        ask("who should I take?"),
        answer("Take **Bijan**.\n\n- he is the last tier-1 back\n- `survival 12%`"),
      ]),
    },
  });
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Chat" }).click();
  const entries = page.locator("#chat-list .entry");
  await expect(entries.first()).toContainText("Rob's iPhone asked");
  await expect(entries.first()).toContainText("phone");
  await expect(entries.first()).toContainText("ago");
  await expect(entries.nth(1)).toContainText("Answer for Rob's iPhone");
  await expect(entries.nth(1)).toContainText("$0.02");
  await expect(entries.nth(1).locator("strong")).toHaveText("Bijan");
  await expect(entries.nth(1).locator("li")).toHaveCount(2);
  await expect(entries.nth(1).locator("li code")).toHaveText("survival 12%");
});

test("markdown is text, never markup", async ({ page }) => {
  const host = backend({
    chat: { draft: thread("draft", [answer("<img src=x onerror=alert(1)> is not a tag")]) },
  });
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Chat" }).click();
  await expect(page.locator("#chat-list .entry")).toContainText("<img src=x onerror=alert(1)>");
  await expect(page.locator("#chat-list img")).toHaveCount(0);
});

test("the composer says Answering while the host is busy", async ({ page }) => {
  const host = backend({ chat: { draft: thread("draft", [ask("who?")], true) } });
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Chat" }).click();
  await expect(page.getByRole("button", { name: "Answering…" })).toBeDisabled();
  await expect(page.getByLabel("Ask the assistant")).toBeDisabled();

  // The answer lands over the socket: the composer opens again.
  await emit(page, {
    type: "shared-chat",
    payload: thread("draft", [ask("who?"), answer("Bijan.")], false),
  });
  await expect(page.getByRole("button", { name: "Send" })).toBeEnabled();
  await expect(page.locator("#chat-list .entry")).toHaveCount(2);
});

test("a busy host answers 409 and the page says so inline", async ({ page }) => {
  const host = backend({ chat: { draft: thread("draft", []) }, postStatus: 409 });
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Chat" }).click();
  await page.getByLabel("Ask the assistant").fill("who should I take?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.locator("#chat-note")).toHaveText("The host is still answering.");

  host.postStatus = 429;
  await page.getByLabel("Ask the assistant").fill("and now?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.locator("#chat-note")).toContainText("Too many questions");
});

test("a draft-updated frame repaints the board", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page);
  const view = host.draft as {
    draft: Record<string, unknown>;
    recommendations: Record<string, unknown>[];
  };
  await expect(page.locator("#clock-strip")).toContainText(
    `Pick ${String(view.draft.current_pick)}`,
  );
  const moved = {
    ...view,
    draft: { ...view.draft, current_pick: 99, is_my_pick: false, on_clock_name: "Dana" },
    recommendations: [
      { ...view.recommendations[0], name: "Somebody Else", reasons: ["newly top"] },
    ],
  };
  await emit(page, { type: "draft-updated", payload: moved });
  await expect(page.locator("#clock-strip")).toContainText("Pick 99");
  await expect(page.locator("#clock-strip")).toContainText("Dana");
  await expect(page.locator("#clock-strip")).not.toContainText("Your pick");
  await expect(page.locator("#recs .name").first()).toHaveText("Somebody Else");
});

test("the page pings the host every twenty-five seconds", async ({ page }) => {
  await serve(page, backend());
  await page.clock.install();
  await pair(page);
  await page.clock.runFor(100);
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __socketUrl: string }).__socketUrl))
    .toContain("/api/events?token=tok-1");
  await page.clock.runFor(26_000);
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __sent: string[] }).__sent))
    .toContainEqual(JSON.stringify({ type: "ping" }));
});

test("the Week tab appears only once the host has a season loaded", async ({ page }) => {
  const season = dump("dev-season-fixture.json");
  const host = backend({ season, chat: { season: thread("season", [ask("start him?")]) } });
  await serve(page, host);
  await pair(page);
  const week = page.getByRole("button", { name: "Week" });
  await expect(week).toBeVisible();
  await week.click();
  await expect(page.locator("#week-header")).toContainText(`Week ${String(season.week)}`);
  await expect(page.locator("#week-calls li")).not.toHaveCount(0);
  // The Week tab carries the season thread, not the draft one.
  await expect(page.locator("#chat-list .entry")).toContainText("Rob's iPhone asked");
  await expect(page.locator("#chat-list .entry")).toContainText("start him?");
});

test("the Week tab is hidden when the host has no season", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await expect(page.getByRole("button", { name: "Week" })).toBeHidden();
});

test("a revoked frame sends the phone back to the pair screen", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await emit(page, { type: "revoked", payload: {} });
  await expect(page.getByRole("alert")).toContainText("Pairing was revoked");
  await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
  expect(await page.evaluate(() => window.localStorage.getItem("da.companion.token"))).toBeNull();
});

test("a 401 on any request sends the phone back to the pair screen", async ({ page }) => {
  const host = backend({ chat: { draft: thread("draft", []) } });
  await serve(page, host);
  await pair(page);
  await page.getByRole("button", { name: "Chat" }).click();
  host.authorised = false;
  await page.getByLabel("Ask the assistant").fill("still there?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("alert")).toContainText("Pairing was revoked");
  await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
  // The host name it already knew is still on screen.
  await expect(page.locator("#pair-host")).toHaveText("Hosted by Justin's Mac");
});

test("the reconnecting pill shows while the socket is down", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await expect(page.locator("#clock-strip")).toContainText("Pick");
  await expect(page.locator("#reconnect-pill")).toBeHidden();
  // Drop the connection and refuse the retries, the way a phone that has
  // walked out of Wi-Fi range sees it.
  await page.evaluate(() => {
    const store = window as unknown as { __stayDown: boolean; __drop: () => void };
    store.__stayDown = true;
    store.__drop();
  });
  await expect(page.locator("#reconnect-pill")).toBeVisible();
});

test("a poll-health frame updates the sync line", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await emit(page, {
    type: "poll-health",
    payload: { last_success_at: null, consecutive_failures: 3, last_error: "timeout" },
  });
  await expect(page.locator("#health")).toContainText("3 failed syncs");
});

test("works at 360px without the page scrolling sideways", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await serve(page, backend());
  await pair(page);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(1);
  // Every tab target is at least the 44px Apple and Google both ask for.
  for (const name of ["Now", "Picks", "Chat"]) {
    const box = await page.getByRole("button", { name }).boundingBox();
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
  }
});
