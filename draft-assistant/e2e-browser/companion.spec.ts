import { expect, test, type Page } from "@playwright/test";
import { answer, ask, backend, emit, pair, serve, thread } from "./companionServer";
import { dump } from "./fixtures";

/**
 * The phone companion page, driven by a real browser.
 *
 * The page is a handful of static files that the Rust host serves at `/` and
 * `/static/*`; nothing builds them, so `companionServer.ts` serves them out
 * of `src-tauri/companion-static/` with the same Content-Security-Policy the
 * host sets. That policy is the point of serving them rather than pasting
 * markup into the test: if anything inline ever creeps into the page, the
 * browser refuses to run it here exactly as it would on a phone.
 *
 * The host's WebSocket is replaced by a controllable fake, so a test can push
 * a `draft-updated` or `shared-chat` frame at the exact moment it wants one.
 */

/**
 * A locator that matches only while the element is actually on screen.
 *
 * The page hides whole panels with the `hidden` attribute, and Playwright's
 * text assertions wait for an element to be *attached*, not visible: a
 * regression that filled the Picks panel while the Now panel was showing used
 * to satisfy every one of them.
 */
const shown = (page: Page, selector: string) => page.locator(`${selector}:visible`);

test("refuses the wrong code and opens the draft on the right one", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page, "000000");
  await expect(page.getByRole("alert")).toHaveText("That code did not work.");
  await expect(page.getByRole("button", { name: "Now" })).toBeHidden();

  await page.getByLabel("Pairing code").fill(host.code);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  await expect(page.getByRole("button", { name: "Now" })).toHaveAttribute("aria-current", "page");
  // The fixture has us on the clock, and the host's name is now known.
  await expect(shown(page, "#clock-strip")).toContainText("Your pick");
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
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
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
  const card = shown(page, "#recs .card").first();
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
  const rows = shown(page, "#picks .row");
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
  const entries = shown(page, "#chat-list .entry");
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
  await expect(shown(page, "#chat-list .entry")).toContainText("<img src=x onerror=alert(1)>");
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
  // The question the host did not take is still in the box, not gone with
  // the note beside an empty one.
  await expect(page.getByLabel("Ask the assistant")).toHaveValue("who should I take?");

  host.postStatus = 429;
  await page.getByLabel("Ask the assistant").fill("and now?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(shown(page, "#chat-note")).toContainText("Too many questions");
});

test("a draft-updated frame repaints the board", async ({ page }) => {
  const host = backend();
  await serve(page, host);
  await pair(page);
  const view = host.draft as {
    draft: Record<string, unknown>;
    recommendations: Record<string, unknown>[];
  };
  await expect(shown(page, "#clock-strip")).toContainText(
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
  await expect(shown(page, "#clock-strip")).toContainText("Pick 99");
  await expect(shown(page, "#clock-strip")).toContainText("Dana");
  await expect(shown(page, "#clock-strip")).not.toContainText("Your pick");
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

test("a ping the host never answers drops the socket and says so", async ({ page }) => {
  await serve(page, backend());
  await page.clock.install();
  await pair(page);
  await page.clock.runFor(100);
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __socketUrl: string }).__socketUrl))
    .toContain("/api/events?token=tok-1");
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  await expect(page.locator("#reconnect-pill")).toBeAttached();
  await expect(page.locator("#reconnect-pill")).toBeHidden();
  // The failure this prevents: the page pinged and never read the reply, so a
  // socket the phone's network had quietly dropped stayed "open" for ever and
  // the page went on showing a draft that had stopped arriving.
  await page.evaluate(() => {
    (window as unknown as { __stayDown: boolean }).__stayDown = true;
  });
  await page.clock.runFor(3 * 25_000 + 1_000);
  await expect(page.locator("#reconnect-pill")).toBeVisible();
});

test("the pick clock counts down by the host's clock, not the phone's", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  // This phone is four minutes behind the host. Without the offset the 45
  // second pick clock would read as long over.
  const board = dump("dev-fixture.json") as { draft: Record<string, unknown> };
  await emit(page, { type: "hello", payload: { server_now_ms: Date.now() + 240_000 } });
  board.draft.clock_deadline_ms = Date.now() + 240_000 + 45_000;
  await emit(page, { type: "draft-updated", payload: board });
  await expect(shown(page, "#clock-strip")).toContainText("0:4");
});

test("the Week tab appears only once the host has a season loaded", async ({ page }) => {
  const season = dump("dev-season-fixture.json");
  const host = backend({ season, chat: { season: thread("season", [ask("start him?")]) } });
  await serve(page, host);
  await pair(page);
  const week = page.getByRole("button", { name: "Week" });
  await expect(week).toBeVisible();
  await week.click();
  await expect(shown(page, "#week-header")).toContainText(`Week ${String(season.week)}`);
  await expect(page.locator("#week-calls li")).not.toHaveCount(0);
  // The Week tab carries the season thread, not the draft one.
  await expect(shown(page, "#chat-list .entry")).toContainText("Rob's iPhone asked");
  await expect(shown(page, "#chat-list .entry")).toContainText("start him?");
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
  await expect(page.getByRole("alert")).toContainText("The host restarted or revoked this device");
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
  await expect(page.getByRole("alert")).toContainText("The host restarted or revoked this device");
  await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
  // The host name it already knew is still on screen.
  await expect(page.locator("#pair-host")).toHaveText("Hosted by Justin's Mac");
});

test("a socket closed with 4401 asks for the code instead of retrying", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  // What a restarted host does to a phone holding a token it has forgotten:
  // the page used to reconnect for ever against a socket that always refused.
  await page.evaluate(() => {
    const store = window as unknown as { __stayDown: boolean; __drop: (code?: number) => void };
    store.__stayDown = true;
    store.__drop(4401);
  });
  await expect(page.getByRole("alert")).toContainText("The host restarted or revoked this device");
  await expect(page.getByRole("button", { name: "Connect" })).toBeVisible();
  await expect(page.locator("#reconnect-pill")).toBeAttached();
  await expect(page.locator("#reconnect-pill")).toBeHidden();
});

test("the reconnecting pill shows while the socket is down", async ({ page }) => {
  await serve(page, backend());
  await pair(page);
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  await expect(page.locator("#reconnect-pill")).toBeAttached();
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
  await expect(shown(page, "#health")).toContainText("3 failed syncs");
});

test("serves every file the page loads, the touch icon and manifest included", async ({ page }) => {
  const missing: string[] = [];
  // Only the page's own files: the host answers 404 for a season or a thread
  // it has not loaded, and the page reads that as "nothing there".
  page.on("response", (response) => {
    const { pathname } = new URL(response.url());
    if (response.status() >= 400 && (pathname === "/" || pathname.startsWith("/static/"))) {
      missing.push(`${response.status()} ${pathname}`);
    }
  });
  await serve(page, backend());
  await pair(page);
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  // The two files the browser fetches only on install, asked for by hand.
  const touchIcon = page.locator('link[rel="apple-touch-icon"]');
  await expect(touchIcon).toHaveAttribute("href", /\.png$/);
  const manifest = page.locator('link[rel="manifest"]');
  await expect(manifest).toHaveAttribute("href", /manifest\.webmanifest$/);
  const hrefs = [
    (await touchIcon.getAttribute("href")) ?? "",
    (await manifest.getAttribute("href")) ?? "",
    "/static/sw.js",
  ];
  const fetched = await page.evaluate(async (paths: string[]) => {
    const out: Record<string, string> = {};
    for (const href of paths) {
      const response = await fetch(href);
      out[href] = `${response.status} ${response.headers.get("content-type") ?? ""}`;
    }
    return out;
  }, hrefs);
  expect(fetched).toEqual({
    "/static/apple-touch-icon.png": "200 image/png",
    "/static/manifest.webmanifest": "200 application/manifest+json",
    "/static/sw.js": "200 text/javascript",
  });
  expect(missing).toEqual([]);
});

test("after pairing, the address carries the identity an installed copy pairs with", async ({
  page,
}) => {
  // iOS gives a home-screen web app its own storage, so the token, id and
  // name Safari saved never reach it. The id and name travel in the address
  // that Add to Home Screen bookmarks; the token stays out of it.
  await serve(page, backend());
  await page.goto("/");
  await page.getByLabel("This device").fill("Rob's iPhone");
  await page.getByLabel("Pairing code").fill("424242");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  const url = new URL(page.url());
  expect(url.searchParams.get("device")).toBe("dev-1");
  expect(url.searchParams.get("name")).toBe("Rob's iPhone");
  expect(url.href).not.toContain("tok-1");

  // The installed copy: same address, empty storage. The form offers the
  // same name, and the pair request names the same device.
  await page.evaluate(() => window.localStorage.clear());
  const sentIds: unknown[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/api/pair")) {
      sentIds.push((request.postDataJSON() as { device_id?: unknown }).device_id);
    }
  });
  await page.goto(url.href);
  await expect(page.getByLabel("This device")).toHaveValue("Rob's iPhone");
  await page.getByLabel("Pairing code").fill("424242");
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(shown(page, "#clock-strip")).toContainText("Pick");
  expect(sentIds).toEqual(["dev-1"]);
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
