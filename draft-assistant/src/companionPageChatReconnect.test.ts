import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";
import { boot, FakeSocket, flush, okJson } from "./test/companionPageHarness";

/**
 * Three small things on the companion phone page: the chat list while the
 * host is still answering ("Thinking..." and the scroll to a new answer), the
 * Reconnecting pill that retries on a tap instead of waiting out its backoff,
 * and boot.js's Reload button for a page whose scripts never arrived.
 * The nudge itself (toast, Alerts button, buzz) is in
 * companionPageAlerts.test.ts.
 */

const asset = (file: string): string =>
  readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8");

/** A live draft that is someone else's pick, so no toast gets in the way. */
const board = {
  draft: {
    status: "drafting",
    teams: 12,
    rounds: 15,
    current_pick: 12,
    current_round: 1,
    on_clock_slot: 12,
    my_slot: 5,
    is_my_pick: false,
    total_picks_made: 11,
    keeper_picks: [],
  },
  recommendations: [],
  recent_picks: [],
  rosters: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: 0 },
};
const entry = (role: "user" | "assistant", text: string) => ({
  role,
  text,
  at_ms: Date.now() - 5_000,
  device: { name: "Rob's iPhone", kind: "phone" },
  cost_usd: null,
});
const thread = (entries: unknown[], busy = false) => ({
  league_id: "league-1",
  screen: "draft",
  busy,
  entries,
});

afterEach(() => {
  document.body.innerHTML = "";
});

/** A paired page with its socket open and a draft loaded, on the given tab. */
async function paired(tabName = "now") {
  const page = boot(() => okJson(null));
  await flush();
  const socket = FakeSocket.instances[0];
  if (!socket) throw new Error("no socket");
  socket.open();
  socket.frame("draft-updated", board);
  tab(tabName).click();
  return { ...page, socket };
}
const tab = (name: string): HTMLButtonElement => {
  const button = document.querySelector<HTMLButtonElement>(`[data-tab="${name}"]`);
  if (!button) throw new Error(`no ${name} tab`);
  return button;
};

describe("the chat list", () => {
  const scrolled: Mock<(options?: unknown) => void> = vi.fn();
  afterEach(() => {
    scrolled.mockClear();
    delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView;
  });

  it("shows Thinking... where the answer will land, instead of Nothing asked yet", async () => {
    const { byId, socket } = await paired("chat");
    const list = byId("chat-list");
    expect(list.textContent).toContain("Nothing asked yet.");
    socket.frame("shared-chat", thread([], true));
    expect(list.textContent).not.toContain("Nothing asked yet.");
    const thinking = list.querySelector("li.entry.assistant.thinking");
    expect(thinking?.textContent).toBe("Thinking…");
    expect(list.lastElementChild).toBe(thinking);
    expect((byId("chat-input") as HTMLInputElement).disabled).toBe(true);
    // The answer arrives: the placeholder goes with the busy flag.
    socket.frame("shared-chat", thread([entry("user", "Who?"), entry("assistant", "Him.")]));
    expect(list.querySelector(".thinking")).toBeNull();
    expect(list.children).toHaveLength(2);
    expect((byId("chat-input") as HTMLInputElement).disabled).toBe(false);
  });

  it("keeps Thinking... under the question the host is answering", async () => {
    const { byId, socket } = await paired("chat");
    socket.frame("shared-chat", thread([entry("user", "Who should I take?")], true));
    const items = [...byId("chat-list").children];
    expect(items).toHaveLength(2);
    expect(items[0]?.textContent).toContain("Who should I take?");
    expect(items[1]?.className).toBe("entry assistant thinking");
  });

  it("brings a new answer into view when the Chat tab is showing", async () => {
    // jsdom has no layout and no scrollIntoView; the page has to call it.
    Element.prototype.scrollIntoView = scrolled;
    const { byId, socket } = await paired("chat");
    socket.frame("shared-chat", thread([entry("user", "Who should I take?")]));
    expect(scrolled).toHaveBeenCalledTimes(1);
    scrolled.mockClear();
    const grown = thread([entry("user", "Who should I take?"), entry("assistant", "Bijan.")]);
    socket.frame("shared-chat", grown);
    expect(scrolled).toHaveBeenCalledTimes(1);
    expect(scrolled.mock.contexts[0]).toBe(byId("chat-list").lastElementChild);
    expect(scrolled).toHaveBeenCalledWith({ block: "end" });
    // A repaint that changes nothing in the thread does not scroll again.
    socket.frame("poll-health", { consecutive_failures: 0 });
    expect(scrolled).toHaveBeenCalledTimes(1);
    // Nor does a thread that only lost its busy flag: nothing new to see.
    socket.frame("shared-chat", grown);
    expect(scrolled).toHaveBeenCalledTimes(1);
  });

  it("does not scroll a panel that is not on screen", async () => {
    Element.prototype.scrollIntoView = scrolled;
    const { socket } = await paired("chat");
    socket.frame("shared-chat", thread([entry("user", "Who?")]));
    scrolled.mockClear();
    tab("now").click();
    socket.frame("shared-chat", thread([entry("user", "Who?"), entry("assistant", "Him.")]));
    expect(scrolled).not.toHaveBeenCalled();
  });
});

describe("the Reconnecting pill", () => {
  it("retries on a tap rather than waiting out the backoff", async () => {
    const { byId, socket, fetch, fireTimers } = await paired();
    const pill = byId("reconnect-pill");
    expect(pill.hidden).toBe(true);
    socket.drop();
    expect(pill.hidden).toBe(false);
    fetch.mockClear();
    pill.click();
    await flush();
    expect(fetch.mock.calls.map(([path]) => path)).toContain("/api/state");
    expect(FakeSocket.instances).toHaveLength(2);
    expect(FakeSocket.instances[1]?.url).toBe("ws://192.168.1.20:7878/api/events?token=tok-1");
    // The backoff timer from the drop was cancelled: firing what is left
    // opens no third socket beside the one the tap made.
    fireTimers();
    await flush();
    expect(FakeSocket.instances).toHaveLength(2);
    expect(FakeSocket.instances[1]?.closed).toBe(false);
    FakeSocket.instances[1]?.open();
    expect(pill.hidden).toBe(true);
  });

  it("a tap during a retry lets go of the socket that was half open", async () => {
    const { byId, socket, fireTimers } = await paired();
    socket.drop();
    fireTimers();
    await flush();
    const retry = FakeSocket.instances[1];
    expect(retry).toBeDefined();
    expect(retry?.readyState).toBe(0);
    byId("reconnect-pill").click();
    await flush();
    // One socket at a time: the one still connecting is closed first.
    expect(retry?.closed).toBe(true);
    expect(FakeSocket.instances).toHaveLength(3);
  });
});

describe("boot.js", () => {
  it("offers a Reload button when the page's scripts never arrived", () => {
    // boot.js is not among the scripts the harness runs: it is evaluated
    // here over the shipped markup, with its ten second timer held.
    const page = asset("index.html");
    document.body.innerHTML = page.slice(
      page.indexOf('<div id="companion-root">'),
      page.indexOf("<script"),
    );
    let giveUp: (() => void) | null = null;
    const reload = vi.fn();
    const window = {
      location: { reload },
      addEventListener: () => undefined,
      setTimeout: (fn: () => void) => {
        giveUp = fn;
        return 1;
      },
    };
    runInNewContext(asset("boot.js"), { window, document });
    const button = document.getElementById("boot-reload") as HTMLButtonElement;
    expect(button.hidden).toBe(true);
    giveUp!();
    expect(button.hidden).toBe(false);
    expect(document.getElementById("boot-status")?.textContent).toContain("Reload this page");
    button.click();
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
