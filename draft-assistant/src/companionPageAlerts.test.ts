import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext, runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";
import { boot, FakeSocket, flush, okJson, TOKEN_KEY } from "./test/companionPageHarness";

/**
 * The companion phone page's nudge: the pure decision in
 * `src-tauri/companion-static/alerts.js` and the page around it: the toast,
 * the Alerts button, the buzz and the chirp. The chat list and the
 * Reconnecting pill that landed beside it are in
 * companionPageChatReconnect.test.ts.
 *
 * The harness window has no `navigator` and no `AudioContext`, which is what
 * the page sees on an iPhone over plain http; the tests that need a phone
 * that can buzz boot the same shipped scripts over a window of their own.
 */

interface Memory {
  pick: number | null;
  low: number | null;
}
interface Alert {
  kind: "your-turn" | "low-time";
  pick: number;
}
interface Alerts {
  LOW_SECS: number;
  nextAlert(
    this: void,
    memory: Memory | null,
    view: unknown,
    nowMs: number,
  ): { memory: Memory; alert: Alert | null };
  chirp(this: void, ctx: unknown, kind: string): boolean;
}

const asset = (file: string): string =>
  readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8");
const loadAlerts = (): Alerts => {
  const window = { Companion: {} };
  runInNewContext(asset("alerts.js"), { window });
  return window.Companion as Alerts;
};
const NOW = 1_700_000_000_000;

/** A draft view as the host sends it: my pick is up with a full minute left. */
const view = (draft: Record<string, unknown> = {}) => ({
  draft: {
    status: "drafting",
    teams: 12,
    rounds: 15,
    pick_timer: 60,
    current_pick: 5,
    current_round: 1,
    on_clock_slot: 5,
    on_clock_name: "Me",
    my_slot: 5,
    is_my_pick: true,
    picks_until_mine: 0,
    my_next_picks: [5, 20],
    total_picks_made: 4,
    paused: false,
    clock_deadline_ms: NOW + 60_000,
    pick_slot_overrides: {},
    keeper_picks: [],
    is_auction: false,
    ...draft,
  },
  recommendations: [],
  recent_picks: [],
  rosters: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: 0 },
});
/** The same view with the clock read against the real `Date.now()` the page uses. */
const live = (draft: Record<string, unknown> = {}, leftMs = 60_000) =>
  view({ clock_deadline_ms: Date.now() + leftMs, ...draft });

describe("nextAlert", () => {
  const { nextAlert, LOW_SECS } = loadAlerts();

  it.each([
    ["it is not my pick", { is_my_pick: false }],
    ["the draft is complete", { status: "complete" }],
    ["the draft is paused", { paused: true }],
    ["the status says paused", { status: "paused" }],
    ["the draft has not started", { status: "pre_draft", total_picks_made: 0 }],
    ["only keepers are in", { status: "pre_draft", total_picks_made: 2, keeper_picks: [1, 2] }],
    ["it is an auction", { is_auction: true }],
    ["the pick number is missing", { current_pick: null }],
  ])("says nothing when %s", (_label, change) => {
    const { memory, alert } = nextAlert(null, view(change), NOW);
    expect(alert).toBeNull();
    expect(memory).toEqual({ pick: null, low: null });
  });

  it("says nothing with no draft loaded", () => {
    expect(nextAlert(null, null, NOW).alert).toBeNull();
    expect(nextAlert(null, { draft: null }, NOW).alert).toBeNull();
  });

  it("nudges once when my pick comes up, not again on the same pick", () => {
    const first = nextAlert(null, view(), NOW);
    expect(first.alert).toEqual({ kind: "your-turn", pick: 5 });
    expect(first.memory).toEqual({ pick: 5, low: null });
    // The page repaints every second: the same snapshot again is silent.
    const again = nextAlert(first.memory, view(), NOW + 1_000);
    expect(again.alert).toBeNull();
    expect(again.memory).toBe(first.memory);
  });

  it("nudges once more when the clock is under fifteen seconds", () => {
    const spoken = nextAlert(null, view(), NOW).memory;
    const deadline = NOW + LOW_SECS * 1000;
    // Sixteen seconds left is still calm.
    expect(nextAlert(spoken, view({ clock_deadline_ms: deadline }), NOW - 1_000).alert).toBeNull();
    const low = nextAlert(spoken, view({ clock_deadline_ms: deadline }), NOW);
    expect(low.alert).toEqual({ kind: "low-time", pick: 5 });
    expect(low.memory).toEqual({ pick: 5, low: 5 });
    // Once; the next tick with the clock still low says nothing.
    const tick = nextAlert(low.memory, view({ clock_deadline_ms: deadline }), NOW + 1_000);
    expect(tick.alert).toBeNull();
    // And a host that sends no clock gets no low-time at all.
    expect(nextAlert(spoken, view({ clock_deadline_ms: null }), NOW).alert).toBeNull();
  });

  it.each([0, -1_000])("stays quiet when the clock reads %d ms: the pick has expired", (left) => {
    const spoken = nextAlert(null, view(), NOW).memory;
    const late = nextAlert(spoken, view({ clock_deadline_ms: NOW + left }), NOW);
    expect(late.alert).toBeNull();
    expect(late.memory).toBe(spoken);
  });

  it("a new pick number starts both nudges over", () => {
    let memory = nextAlert(null, view(), NOW).memory;
    memory = nextAlert(memory, view({ clock_deadline_ms: NOW + 5_000 }), NOW).memory;
    expect(memory).toEqual({ pick: 5, low: 5 });
    const next = nextAlert(memory, view({ current_pick: 20 }), NOW);
    expect(next.alert).toEqual({ kind: "your-turn", pick: 20 });
    expect(next.memory).toEqual({ pick: 20, low: null });
    const soon = view({ current_pick: 20, clock_deadline_ms: NOW + 5_000 });
    expect(nextAlert(next.memory, soon, NOW).alert).toEqual({ kind: "low-time", pick: 20 });
  });
});

/** An `AudioContext` that counts the notes it was asked to play. */
class FakeAudioContext {
  static made: FakeAudioContext[] = [];
  started: number[] = [];
  currentTime = 0;
  destination = {};
  constructor(public state: "running" | "suspended" = "running") {
    FakeAudioContext.made.push(this);
  }
  resume(): Promise<void> {
    this.state = "running";
    return Promise.resolve();
  }
  createOscillator() {
    const osc = {
      type: "",
      frequency: { value: 0 },
      connect: (node: unknown) => node,
      start: () => void this.started.push(osc.frequency.value),
      stop: () => undefined,
    };
    return osc;
  }
  createGain() {
    return {
      gain: { setValueAtTime: () => undefined, exponentialRampToValueAtTime: () => undefined },
      connect: (node: unknown) => node,
    };
  }
}

describe("chirp", () => {
  it("plays two notes for a turn and three for a low clock, only on a running context", () => {
    const { chirp } = loadAlerts();
    const running = new FakeAudioContext();
    expect(chirp(running, "your-turn")).toBe(true);
    expect(running.started).toEqual([660, 880]);
    expect(chirp(running, "low-time")).toBe(true);
    expect(running.started).toEqual([660, 880, 880, 880, 880]);
    // iPhone before any gesture: the context exists but is not allowed yet.
    const asleep = new FakeAudioContext("suspended");
    expect(chirp(asleep, "your-turn")).toBe(false);
    expect(asleep.started).toEqual([]);
    expect(chirp(null, "your-turn")).toBe(false);
  });
});

afterEach(() => {
  document.body.innerHTML = "";
  FakeAudioContext.made = [];
});

/** A paired page on the Now tab, its socket open, before any draft arrives. */
async function paired(saved?: Record<string, string>) {
  const page = boot(() => okJson(null), saved ? { saved } : {});
  await flush();
  const socket = FakeSocket.instances[0];
  if (!socket) throw new Error("no socket");
  socket.open();
  return { ...page, socket };
}
const tab = (name: string): HTMLButtonElement => {
  const button = document.querySelector<HTMLButtonElement>(`[data-tab="${name}"]`);
  if (!button) throw new Error(`no ${name} tab`);
  return button;
};

describe("the toast", () => {
  it("shows when my pick comes up, goes away on its own, and a tap opens the Now tab", async () => {
    const { byId, socket, fireTimers } = await paired();
    const toast = byId("alert-toast");
    expect(toast.hidden).toBe(true);
    socket.frame("draft-updated", live());
    expect(toast.hidden).toBe(false);
    expect(toast.textContent).toBe("Your pick is up");
    expect(toast.dataset.kind).toBe("your-turn");
    // A repaint on the same pick does not restart it or show it twice.
    socket.frame("poll-health", { consecutive_failures: 0 });
    expect(toast.textContent).toBe("Your pick is up");
    fireTimers();
    expect(toast.hidden).toBe(true);
    // Reading the picks list when the next turn lands: the toast is the way back.
    tab("picks").click();
    expect(tab("picks").getAttribute("aria-current")).toBe("page");
    socket.frame("draft-updated", live({ current_pick: 20 }));
    expect(toast.hidden).toBe(false);
    toast.click();
    expect(toast.hidden).toBe(true);
    expect(tab("now").getAttribute("aria-current")).toBe("page");
    expect(tab("picks").getAttribute("aria-current")).toBeNull();
    expect(byId("tab-now").hidden).toBe(false);
  });

  it("says the clock is low, once, when the host's time is nearly gone", async () => {
    const { byId, socket, fireTimers } = await paired();
    const toast = byId("alert-toast");
    socket.frame("draft-updated", live());
    fireTimers();
    socket.frame("draft-updated", live({}, 10_000));
    expect(toast.hidden).toBe(false);
    expect(toast.textContent).toBe("Under 15 seconds on your pick");
    expect(toast.dataset.kind).toBe("low-time");
    fireTimers();
    socket.frame("draft-updated", live({}, 9_000));
    expect(toast.hidden).toBe(true);
  });
});

describe("the Alerts button", () => {
  it("starts on, flips its label and aria-pressed, and remembers the choice", async () => {
    const { byId, stored } = await paired();
    const toggle = byId("alerts-toggle");
    expect(toggle.textContent).toBe("Alerts on");
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(stored("da.companion.alerts")).toBeNull();
    toggle.click();
    expect(toggle.textContent).toBe("Alerts off");
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(stored("da.companion.alerts")).toBe("off");
    toggle.click();
    expect(toggle.textContent).toBe("Alerts on");
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(stored("da.companion.alerts")).toBe("on");
  });

  it("comes up off when that is what was chosen last time", async () => {
    const { byId } = await paired({ [TOKEN_KEY]: "tok-1", "da.companion.alerts": "off" });
    expect(byId("alerts-toggle").textContent).toBe("Alerts off");
    expect(byId("alerts-toggle").getAttribute("aria-pressed")).toBe("false");
  });
});

// ---- a phone that can buzz and make a sound --------------------------------

const page = asset("index.html");
const markup = page.slice(page.indexOf('<div id="companion-root">'), page.indexOf("<script"));
const scripts = [
  "helpers",
  "clock",
  "pwa",
  "models",
  "pictures",
  "restore",
  "roster",
  "signals",
  "week",
  "available",
  "alerts",
  "compact",
  "app",
].map((name) => asset(`${name}.js`));
const OFF = { [TOKEN_KEY]: "tok-1", "da.companion.alerts": "off" };

/** The harness's boot over a window with a `navigator` that vibrates and,
 *  when asked, an `AudioContext`: an Android phone in Chrome. */
async function phone(options: { saved?: Record<string, string>; audio?: boolean } = {}) {
  document.body.innerHTML = markup;
  FakeSocket.instances = [];
  const pending = new Map<number, () => void>();
  let nextTimer = 1;
  const saved = new Map(Object.entries(options.saved ?? { [TOKEN_KEY]: "tok-1" }));
  const vibrate: Mock<(pattern: number[]) => boolean> = vi.fn(() => true);
  const navigator = { userAgent: "Mozilla/5.0 (Linux; Android 14; Pixel 8)", vibrate };
  const window = {
    location: { protocol: "http:", host: "192.168.1.20:7878", pathname: "/", search: "", hash: "" },
    history: { replaceState: () => undefined },
    localStorage: {
      getItem: (key: string) => saved.get(key) ?? null,
      setItem: (key: string, value: string) => void saved.set(key, value),
      removeItem: (key: string) => void saved.delete(key),
    },
    navigator,
    setTimeout: (fn: () => void) => {
      const id = nextTimer;
      nextTimer += 1;
      pending.set(id, fn);
      return id;
    },
    clearTimeout: (id: number) => void pending.delete(id),
    setInterval: () => 0,
    clearInterval: () => undefined,
    addEventListener: () => undefined,
    ...(options.audio ? { AudioContext: FakeAudioContext } : {}),
  };
  const sandbox = { window, document, navigator, fetch: () => okJson(null), WebSocket: FakeSocket };
  const context = createContext({ ...sandbox, URLSearchParams });
  for (const script of scripts) runInContext(script, context);
  await flush();
  const socket = FakeSocket.instances[0];
  if (!socket) throw new Error("no socket");
  socket.open();
  const byId = (id: string) => {
    const node = document.getElementById(id);
    if (!node) throw new Error(`no #${id} on the page`);
    return node;
  };
  const fireTimers = () => {
    const due = [...pending.values()];
    pending.clear();
    for (const fn of due) fn();
  };
  return { socket, vibrate, byId, fireTimers };
}

describe("the buzz", () => {
  it("vibrates a long pattern for my pick and a short one for a low clock", async () => {
    const { socket, vibrate, fireTimers } = await phone();
    socket.frame("draft-updated", live());
    expect(vibrate).toHaveBeenCalledTimes(1);
    expect(vibrate).toHaveBeenLastCalledWith([200, 100, 200, 100, 400]);
    fireTimers();
    socket.frame("draft-updated", live({}, 10_000));
    expect(vibrate).toHaveBeenCalledTimes(2);
    expect(vibrate).toHaveBeenLastCalledWith([120, 60, 120]);
    // The same low clock a second later: the phone is not buzzed again.
    socket.frame("draft-updated", live({}, 9_000));
    expect(vibrate).toHaveBeenCalledTimes(2);
  });

  it("with alerts off the toast still shows but the phone stays still", async () => {
    const { byId, socket, vibrate } = await phone({ saved: OFF });
    socket.frame("draft-updated", live());
    expect(byId("alert-toast").hidden).toBe(false);
    expect(byId("alert-toast").textContent).toBe("Your pick is up");
    expect(vibrate).not.toHaveBeenCalled();
  });

  it("turning alerts off mid-draft silences the next nudge", async () => {
    const { byId, socket, vibrate } = await phone();
    byId("alerts-toggle").click();
    socket.frame("draft-updated", live());
    expect(byId("alert-toast").hidden).toBe(false);
    expect(vibrate).not.toHaveBeenCalled();
  });
});

describe("the sound", () => {
  it("is armed by the first tap on the page and chirps when my pick comes up", async () => {
    const { socket } = await phone({ audio: true });
    expect(FakeAudioContext.made).toHaveLength(0);
    document.dispatchEvent(new Event("pointerdown"));
    expect(FakeAudioContext.made).toHaveLength(1);
    socket.frame("draft-updated", live());
    expect(FakeAudioContext.made[0]?.started).toEqual([660, 880]);
    // One context for the page: a second tap does not make another.
    document.dispatchEvent(new Event("pointerdown"));
    expect(FakeAudioContext.made).toHaveLength(1);
  });

  it("turning alerts on is the gesture, and the proof is a short chirp", async () => {
    const { byId } = await phone({ audio: true, saved: OFF });
    // A tap while off arms nothing: the phone was asked for silence.
    document.dispatchEvent(new Event("pointerdown"));
    expect(FakeAudioContext.made).toHaveLength(0);
    byId("alerts-toggle").click();
    expect(FakeAudioContext.made).toHaveLength(1);
    expect(FakeAudioContext.made[0]?.started).toEqual([660, 880]);
  });
});
