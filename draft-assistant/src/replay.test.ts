import { afterEach, describe, expect, it, vi } from "vitest";
import type { PollHealth } from "./types";
import { ReplayFeed, readDump, replaySource } from "./replay";

interface Dump {
  generated_at: number;
  tag?: string;
}

const spec = (source: { url: string; live: boolean }) => ({
  source,
  missing: "dev fixture missing (browser preview only works with public/dev-fixture.json)",
  what: "draft state",
  validate: (value: Dump) => value,
  generatedAt: (value: Dump) => value.generated_at,
});

/** A fetch that answers with whatever the queue hands out next. */
function serve(...answers: (Dump | "html" | number)[]) {
  let at = 0;
  const fetcher = vi.fn(() => {
    const answer = answers[Math.min(at, answers.length - 1)];
    at += 1;
    if (typeof answer === "number") {
      return Promise.resolve({ ok: false, status: answer, json: () => Promise.resolve(null) });
    }
    if (answer === "html") {
      // What a dev server actually does with a path it does not know.
      return Promise.resolve({ ok: true, json: () => Promise.reject(new SyntaxError("<")) });
    }
    return Promise.resolve({ ok: true, json: () => Promise.resolve(answer) });
  });
  vi.stubGlobal("fetch", fetcher);
  return fetcher;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("replaySource", () => {
  it("falls back to the checked-in fixture, and only a real parameter goes live", () => {
    expect(replaySource("", "replay", "/dev-fixture.json")).toEqual({
      url: "/dev-fixture.json",
      live: false,
    });
    expect(replaySource("?other=1", "replay", "/f.json").live).toBe(false);
    // Present but blank is a typo, not a source.
    expect(replaySource("?replay=", "replay", "/f.json")).toEqual({ url: "/f.json", live: false });
    expect(replaySource("?replay=%20%20", "replay", "/f.json").live).toBe(false);
    expect(replaySource("?replay=/live-state.json", "replay", "/f.json")).toEqual({
      url: "/live-state.json",
      live: true,
    });
    expect(replaySource("?replay=%2Flive.json&chat=x", "replay", "/f.json").url).toBe("/live.json");
    expect(replaySource("?replay-season=/s.json", "replay-season", "/f.json").url).toBe("/s.json");
  });
});

describe("readDump", () => {
  it("names the missing fixture, and the replay source that answered badly", async () => {
    serve(404);
    await expect(readDump(spec({ url: "/dev-fixture.json", live: false }))).rejects.toThrow(
      /dev fixture missing/,
    );
    serve(503);
    await expect(readDump(spec({ url: "/live-state.json", live: true }))).rejects.toThrow(
      "replay source /live-state.json returned 503",
    );
  });

  it("says a dev server answered with a page, not the raw parse failure", async () => {
    serve("html");
    await expect(readDump(spec({ url: "/typo.json", live: true }))).rejects.toThrow(
      /could not read draft state from \/typo\.json — it is not a state dump/,
    );
    // The message a reader cannot act on must not be the one they get.
    serve("html");
    await expect(readDump(spec({ url: "/typo.json", live: true }))).rejects.not.toThrow(
      /Unexpected token/,
    );
  });

  it("reads the source uncached, so a rewritten dump is not served from memory", async () => {
    const fetcher = serve({ generated_at: 1 });
    await readDump(spec({ url: "/live.json", live: true }));
    expect(fetcher).toHaveBeenCalledWith("/live.json", { cache: "no-store" });
  });

  it("puts the dump through the caller's validation", async () => {
    serve({ generated_at: 1 });
    await expect(
      readDump({
        ...spec({ url: "/live.json", live: true }),
        validate: () => {
          throw new Error("Incompatible draft data");
        },
      }),
    ).rejects.toThrow(/Incompatible draft data/);
  });
});

describe("ReplayFeed", () => {
  it("reads once and caches when no replay source was given", async () => {
    const fetcher = serve({ generated_at: 1 });
    const feed = new ReplayFeed(spec({ url: "/dev-fixture.json", live: false }));
    expect(feed.live).toBe(false);
    const first = await feed.current();
    expect(await feed.current()).toBe(first);
    // A refresh cannot refresh a file that never changes.
    expect(await feed.refresh()).toBe(first);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("re-reads a live source on every refresh", async () => {
    const fetcher = serve({ generated_at: 1 }, { generated_at: 2 });
    const feed = new ReplayFeed(spec({ url: "/live.json", live: true }));
    expect((await feed.current()).generated_at).toBe(1);
    expect((await feed.refresh()).generated_at).toBe(2);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("pushes only dumps newer than the last, ordered by generated_at", async () => {
    serve({ generated_at: 10 });
    const feed = new ReplayFeed(spec({ url: "/live.json", live: true }));
    const seen: Dump[] = [];
    const health: PollHealth[] = [];
    feed.onView((view) => seen.push(view));
    feed.onHealth((h) => health.push(h));
    await feed.current();

    // Same dump: the file has not been rewritten yet.
    await feed.poll();
    expect(seen).toEqual([]);
    // An older one: a `dump_state` run restarts its own numbering, so only
    // generated_at can say which of two dumps came second.
    serve({ generated_at: 9, tag: "stale" });
    await feed.poll();
    expect(seen).toEqual([]);

    serve({ generated_at: 11, tag: "new" });
    await feed.poll();
    expect(seen).toEqual([{ generated_at: 11, tag: "new" }]);
    expect(health).toEqual([{ last_success_at: 11, consecutive_failures: 0, last_error: null }]);
    // And the pushed dump is what the screens read from then on.
    expect(await feed.current()).toEqual({ generated_at: 11, tag: "new" });
  });

  it("swallows a half-written dump and carries on from the next one", async () => {
    serve({ generated_at: 1 });
    const feed = new ReplayFeed(spec({ url: "/live.json", live: true }));
    const seen: Dump[] = [];
    feed.onView((view) => seen.push(view));
    await feed.current();

    serve("html");
    await expect(feed.poll()).resolves.toBeUndefined();
    serve(500);
    await expect(feed.poll()).resolves.toBeUndefined();
    expect(seen).toEqual([]);

    serve({ generated_at: 2 });
    await feed.poll();
    expect(seen).toEqual([{ generated_at: 2 }]);
  });

  it("polls on the timer until it is stopped", async () => {
    vi.useFakeTimers();
    const fetcher = serve({ generated_at: 1 });
    const feed = new ReplayFeed(spec({ url: "/live.json", live: true }));
    feed.start(1000);
    // Starting twice must not leave two timers behind.
    feed.start(1000);
    await vi.advanceTimersByTimeAsync(3000);
    expect(fetcher).toHaveBeenCalledTimes(3);
    feed.stop();
    await vi.advanceTimersByTimeAsync(3000);
    expect(fetcher).toHaveBeenCalledTimes(3);
    // Stopping a feed that never started is allowed.
    expect(() => feed.stop()).not.toThrow();
  });

  it("stops delivering to a handler that unsubscribed", async () => {
    serve({ generated_at: 1 });
    const feed = new ReplayFeed(spec({ url: "/live.json", live: true }));
    const seen: Dump[] = [];
    const off = feed.onView((view) => seen.push(view));
    const offHealth = feed.onHealth(() => seen.push({ generated_at: -1 }));
    await feed.current();
    off();
    offHealth();
    // Twice: removing a handler that is already gone must be harmless.
    off();
    serve({ generated_at: 2 });
    await feed.poll();
    expect(seen).toEqual([]);
  });
});
