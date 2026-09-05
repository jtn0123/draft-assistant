import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * The companion phone page's live half: the heartbeat that decides a socket
 * is gone, what waking the page up has to rebuild, and the host clock the
 * pick timer counts down by.
 *
 * Loaded the same way `companionPage.test.ts` loads the rest — the real
 * shipped files, in a bare context with a document that owns no page.
 */

interface Timers {
  setInterval: (fn: () => void, ms: number) => number;
  clearInterval: (h: number) => void;
}

interface Companion {
  HEARTBEAT_MS: number;
  MISSED_PONGS: number;
  createHeartbeat(
    timers: Timers,
    hooks: { ping: () => void; silent: () => void; intervalMs?: number },
  ): {
    start(): void;
    pong(): void;
    stop(): void;
    running(): boolean;
    unanswered(): number;
  };
  clockOffset(serverNowMs: unknown, localNowMs: number): number;
  needsRevive(socket: { readyState: number } | null): boolean;
  formatClock(deadlineMs: number | null, nowMs: number): string | null;
  initialState(): { offset: number };
  reduce(state: unknown, action: { type: string; [key: string]: unknown }): { offset: number };
}

const source = ["helpers.js", "clock.js", "app.js"]
  .map((file) => readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8"))
  .join("\n");

const sandbox: { window: { Companion?: Companion }; document: unknown } = {
  window: {},
  document: { readyState: "complete", getElementById: () => null },
};
runInContext(source, createContext(sandbox));
const companion = sandbox.window.Companion as Companion;

describe("the heartbeat", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  /** A heartbeat over fake timers, and what it did. */
  function beating(intervalMs = companion.HEARTBEAT_MS) {
    vi.useFakeTimers();
    const sent: number[] = [];
    let gaveUp = 0;
    const beat = companion.createHeartbeat(
      {
        setInterval: (fn, ms) => globalThis.setInterval(fn, ms) as unknown as number,
        clearInterval: (h) => globalThis.clearInterval(h),
      },
      {
        intervalMs,
        ping: () => sent.push(Date.now()),
        silent: () => {
          gaveUp += 1;
        },
      },
    );
    return { beat, sent, gaveUp: () => gaveUp };
  }

  it("pings on its interval and keeps going while the host answers", () => {
    const { beat, sent, gaveUp } = beating();
    beat.start();
    for (let n = 0; n < 6; n += 1) {
      vi.advanceTimersByTime(companion.HEARTBEAT_MS);
      beat.pong();
    }
    expect(sent).toHaveLength(6);
    expect(gaveUp()).toBe(0);
    expect(beat.running()).toBe(true);
    expect(beat.unanswered()).toBe(0);
  });

  it("gives the socket up when two pings in a row go unanswered", () => {
    // The failure this prevents: the page pinged and never read the reply, so
    // a socket the network had dropped stayed readyState 1 for ever and the
    // page showed a live draft that had stopped arriving.
    const { beat, sent, gaveUp } = beating();
    beat.start();
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * companion.MISSED_PONGS);
    expect(sent).toHaveLength(companion.MISSED_PONGS);
    expect(gaveUp()).toBe(0);
    vi.advanceTimersByTime(companion.HEARTBEAT_MS);
    expect(gaveUp()).toBe(1);
    // And it stops rather than firing again every interval after that.
    expect(beat.running()).toBe(false);
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * 5);
    expect(gaveUp()).toBe(1);
    expect(sent).toHaveLength(companion.MISSED_PONGS);
  });

  it("a late pong clears the count rather than only the last ping", () => {
    const { beat, gaveUp } = beating();
    beat.start();
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * 2);
    expect(beat.unanswered()).toBe(2);
    beat.pong();
    expect(beat.unanswered()).toBe(0);
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * 2);
    expect(gaveUp()).toBe(0);
  });

  it("starting again forgets what the last socket never answered", () => {
    const { beat, gaveUp } = beating();
    beat.start();
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * 2);
    beat.start();
    expect(beat.unanswered()).toBe(0);
    vi.advanceTimersByTime(companion.HEARTBEAT_MS * 2);
    expect(gaveUp()).toBe(0);
    beat.stop();
    expect(beat.running()).toBe(false);
  });
});

describe("coming back from sleep", () => {
  it("rebuilds only a socket that is not open", () => {
    expect(companion.needsRevive(null)).toBe(true);
    // 0 connecting, 2 closing, 3 closed: none of those is a live connection.
    expect(companion.needsRevive({ readyState: 0 })).toBe(true);
    expect(companion.needsRevive({ readyState: 2 })).toBe(true);
    expect(companion.needsRevive({ readyState: 3 })).toBe(true);
    // An open one is left alone: waking the page must not drop a good socket.
    expect(companion.needsRevive({ readyState: 1 })).toBe(false);
  });
});

describe("the host's clock", () => {
  const now = 1_700_000_000_000;

  it("counts a pick down by the host's clock and not the phone's", () => {
    // The phone is four minutes fast. Without the offset a 45 second pick
    // clock reads as already over.
    const offset = companion.clockOffset(now, now + 240_000);
    expect(offset).toBe(-240_000);
    expect(companion.formatClock(now + 45_000, now + 240_000 + offset)).toBe("0:45");
  });

  it("is no offset at all when the host says nothing sensible", () => {
    expect(companion.clockOffset(undefined, now)).toBe(0);
    expect(companion.clockOffset("soon", now)).toBe(0);
    expect(companion.clockOffset(Number.NaN, now)).toBe(0);
  });

  it("is carried on the state so every screen counts down the same", () => {
    const start = companion.initialState();
    expect(start.offset).toBe(0);
    expect(companion.reduce(start, { type: "clock-offset", offset: -240_000 }).offset).toBe(
      -240_000,
    );
  });
});
