import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import { describe, expect, it } from "vitest";

/**
 * The companion phone page's pure logic.
 *
 * `src-tauri/companion-static/app.js` is a plain script served straight to a
 * phone — no build step, no module system — so there is nothing to import.
 * It puts its non-DOM helpers on `window.Companion`, and this file runs the
 * real file in a bare context with a document that owns no page, which is
 * exactly the early return its bootstrap takes. What is exercised here is
 * therefore the shipped file, not a copy of it.
 */

interface Span {
  text: string;
  bold?: boolean;
  code?: boolean;
}

type Block =
  | { type: "p"; spans: Span[] }
  | { type: "ul" | "ol"; items: Span[][] }
  | { type: "code"; text: string };

interface CompanionState {
  screen: string;
  tab: string;
  token: string | null;
  hostName: string | null;
  pairError: string | null;
  draft: unknown;
  season: unknown;
  chat: Record<string, unknown>;
  note: Record<string, unknown>;
  health: unknown;
  connection: string;
}

interface Action {
  type: string;
  [key: string]: unknown;
}

interface Companion {
  REVOKED: string;
  deviceGuess(userAgent?: string): string;
  relativeTime(atMs: number, nowMs: number): string;
  positionClass(position: string | null): string;
  modeLabel(mode: unknown): string;
  collapseAgreeing(
    recs: { player_id: string; mode: string }[],
  ): { player_id: string; mode: string }[];
  backoffDelay(attempt: number): number;
  formatCost(usd: number | null): string | null;
  formatClock(deadlineMs: number | null, nowMs: number): string | null;
  parseMarkdown(text: string | null): Block[];
  initialState(): CompanionState;
  reduce(state: CompanionState, action: Action): CompanionState;
}

// Resolved from the project root: vitest runs with the package as its cwd,
// and under jsdom `import.meta.url` is not a file URL.
// helpers.js publishes `window.Companion`; app.js only reads it back. Both
// run so a helper app.js needs but helpers.js forgot to publish fails here.
const source = ["helpers.js", "app.js"]
  .map((file) => readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8"))
  .join("\n");

const sandbox: { window: { Companion?: Companion }; document: unknown } = {
  window: {},
  document: { readyState: "complete", getElementById: () => null },
};
runInContext(source, createContext(sandbox));
const companion = sandbox.window.Companion as Companion;

describe("naming the device", () => {
  it("guesses from the user agent, and falls back to something honest", () => {
    expect(companion.deviceGuess("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)")).toBe("iPhone");
    expect(companion.deviceGuess("Mozilla/5.0 (iPad; CPU OS 17_0)")).toBe("iPad");
    expect(companion.deviceGuess("Mozilla/5.0 (Linux; Android 14; Pixel 8)")).toBe("Android phone");
    expect(companion.deviceGuess("Mozilla/5.0 (Macintosh)")).toBe("Phone");
    expect(companion.deviceGuess()).toBe("Phone");
  });
});

describe("relative time", () => {
  const now = 1_700_000_000_000;

  it("rounds to the coarsest unit that still says something", () => {
    expect(companion.modeLabel("balanced")).toBe("Balanced");
    expect(companion.modeLabel("upside")).toBe("Upside");
    expect(companion.modeLabel(null)).toBe("");
    const same = [
      { player_id: "a", mode: "balanced" },
      { player_id: "a", mode: "safe" },
      { player_id: "a", mode: "upside" },
    ];
    expect(companion.collapseAgreeing(same)).toEqual([
      { player_id: "a", mode: "Balanced · Safe · Upside agree" },
    ]);
    const split = [
      { player_id: "a", mode: "balanced" },
      { player_id: "b", mode: "safe" },
      { player_id: "a", mode: "upside" },
    ];
    expect(companion.collapseAgreeing(split).map((r) => r.mode)).toEqual([
      "Balanced · Upside agree",
      "Safe",
    ]);
    expect(companion.relativeTime(now - 5_000, now)).toBe("just now");
    expect(companion.relativeTime(now - 44_000, now)).toBe("just now");
    expect(companion.relativeTime(now - 240_000, now)).toBe("4m ago");
    expect(companion.relativeTime(now - 3 * 3600_000, now)).toBe("3h ago");
    expect(companion.relativeTime(now - 2 * 86400_000, now)).toBe("2d ago");
  });

  it("never reads as the future when the two clocks disagree", () => {
    expect(companion.relativeTime(now + 30_000, now)).toBe("just now");
  });
});

describe("position colours", () => {
  it("gives every real position its own class and leaves the rest neutral", () => {
    expect(companion.positionClass("RB")).toBe("pos pos-rb");
    expect(companion.positionClass("def")).toBe("pos pos-def");
    expect(companion.positionClass("LB")).toBe("pos");
    expect(companion.positionClass(null)).toBe("pos");
  });
});

describe("reconnect backoff", () => {
  it("doubles from a second and stops at thirty", () => {
    expect([0, 1, 2, 3, 4, 5, 6, 20].map((n) => companion.backoffDelay(n))).toEqual([
      1000, 2000, 4000, 8000, 16_000, 30_000, 30_000, 30_000,
    ]);
  });
});

describe("costs and clocks", () => {
  it("shows a cost only when there is one, and never rounds it to nothing", () => {
    expect(companion.formatCost(0.0184)).toBe("$0.02");
    expect(companion.formatCost(0.0004)).toBe("$0.0004");
    expect(companion.formatCost(null)).toBeNull();
    // A Claude Code answer costs the cap nothing; "$0.0000" reads as a bug.
    expect(companion.formatCost(0)).toBeNull();
  });

  it("counts a pick clock down, and stops at zero", () => {
    const now = 1_700_000_000_000;
    expect(companion.formatClock(now + 45_000, now)).toBe("0:45");
    expect(companion.formatClock(now + 90_000, now)).toBe("1:30");
    expect(companion.formatClock(now - 10_000, now)).toBe("0:00");
    expect(companion.formatClock(null, now)).toBeNull();
  });
});

describe("the markdown subset", () => {
  it("reads bold and inline code as spans", () => {
    expect(companion.parseMarkdown("Take **Bijan**, `12%` survival")).toEqual([
      {
        type: "p",
        spans: [
          { text: "Take " },
          { text: "Bijan", bold: true },
          { text: ", " },
          { text: "12%", code: true },
          { text: " survival" },
        ],
      },
    ]);
  });

  it("reads both kinds of list", () => {
    const blocks = companion.parseMarkdown("- one\n- two\n\n1. first\n2. second");
    expect(blocks.map((block) => block.type)).toEqual(["ul", "ol"]);
    expect(blocks[0]).toEqual({ type: "ul", items: [[{ text: "one" }], [{ text: "two" }]] });
  });

  it("keeps a fenced block verbatim", () => {
    expect(companion.parseMarkdown("before\n```\na **b**\n```")).toEqual([
      { type: "p", spans: [{ text: "before" }] },
      { type: "code", text: "a **b**" },
    ]);
  });

  it("treats markup as words, since nothing downstream can run it", () => {
    expect(companion.parseMarkdown("<img src=x onerror=alert(1)>")).toEqual([
      { type: "p", spans: [{ text: "<img src=x onerror=alert(1)>" }] },
    ]);
  });

  it("survives nothing at all", () => {
    expect(companion.parseMarkdown("")).toEqual([]);
    expect(companion.parseMarkdown(null)).toEqual([]);
  });
});

describe("the state reducer", () => {
  const start = companion.initialState();

  it("starts on the pair screen with nothing loaded", () => {
    expect(start.screen).toBe("pair");
    expect(start.token).toBeNull();
    expect(start.tab).toBe("now");
  });

  it("pairing keeps the host name it was told", () => {
    const paired = companion.reduce(start, {
      type: "paired",
      token: "tok-1",
      hostName: "Justin's Mac",
    });
    expect(paired.screen).toBe("app");
    expect(paired.token).toBe("tok-1");
    expect(paired.hostName).toBe("Justin's Mac");
    expect(start.screen).toBe("pair");
  });

  it("a 401 drops everything but what the pair screen needs", () => {
    const paired = companion.reduce(start, { type: "paired", token: "t", hostName: "Mac" });
    const loaded = companion.reduce(paired, { type: "draft-updated", payload: { a: 1 } });
    const out = companion.reduce(loaded, { type: "unauthorized" });
    expect(out.screen).toBe("pair");
    expect(out.token).toBeNull();
    expect(out.draft).toBeNull();
    expect(out.hostName).toBe("Mac");
    expect(out.pairError).toBe(companion.REVOKED);
  });

  it("files each chat thread under its own screen", () => {
    const one = companion.reduce(start, {
      type: "shared-chat",
      payload: { screen: "draft", entries: [], busy: false },
    });
    const two = companion.reduce(one, {
      type: "shared-chat",
      payload: { screen: "season", entries: [], busy: true },
    });
    expect(Object.keys(two.chat)).toEqual(["draft", "season"]);
    expect((two.chat.season as { busy: boolean }).busy).toBe(true);
    // A frame with no screen is not a thread, and changes nothing.
    expect(companion.reduce(two, { type: "shared-chat", payload: {} })).toBe(two);
  });

  it("refuses the Week tab until a season is loaded, and gives it up again", () => {
    expect(companion.reduce(start, { type: "tab", tab: "week" })).toBe(start);
    expect(companion.reduce(start, { type: "tab", tab: "nonsense" })).toBe(start);
    const withSeason = companion.reduce(start, { type: "season-updated", payload: { week: 3 } });
    const onWeek = companion.reduce(withSeason, { type: "tab", tab: "week" });
    expect(onWeek.tab).toBe("week");
    const gone = companion.reduce(onWeek, { type: "season-updated", payload: null });
    expect(gone.season).toBeNull();
    expect(gone.tab).toBe("now");
  });

  it("keeps an inline note per screen and ignores what it does not know", () => {
    const noted = companion.reduce(start, { type: "note", screen: "draft", message: "busy" });
    expect(noted.note).toEqual({ draft: "busy" });
    expect(companion.reduce(noted, { type: "who-knows" })).toBe(noted);
  });

  it("tracks the connection for the reconnecting pill", () => {
    const down = companion.reduce(start, { type: "connection", status: "reconnecting" });
    expect(down.connection).toBe("reconnecting");
    expect(companion.reduce(down, { type: "connection", status: "online" }).connection).toBe(
      "online",
    );
  });
});
