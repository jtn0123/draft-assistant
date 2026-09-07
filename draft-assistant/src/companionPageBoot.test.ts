import { afterEach, describe, expect, it } from "vitest";
import {
  boot,
  FakeSocket,
  fakeWakeLock,
  flush,
  okJson,
  type Booted,
} from "./test/companionPageHarness";

/**
 * The companion phone page's DOM and socket half, booted for real.
 *
 * `companionPage.test.ts` runs the shipped files against a document that owns
 * no page, which is the early return the bootstrap takes; nothing there ever
 * reaches `connect()` or the painters. This file boots the same files over
 * the real `index.html` (see `test/companionPageHarness.ts`) and asks about
 * the failures only the live half can have: a host that is away when the
 * page wakes, a retry timer nobody cancelled, a chat list rebuilt under the
 * finger typing into it.
 */

afterEach(() => {
  document.body.innerHTML = "";
});

describe("waking with the host away", () => {
  it("still opens the socket when the first reads fail, so the page keeps retrying", async () => {
    // The failure this prevents: the phone woke, every read threw, and
    // `connect()` sat after the awaits it never reached. "Reconnecting"
    // stayed on screen for ever with no socket behind it.
    const { byId, fetch } = boot(() => Promise.reject(new Error("the host is away")));
    await flush();
    expect(fetch).toHaveBeenCalled();
    expect(FakeSocket.instances).toHaveLength(1);
    expect(FakeSocket.instances[0]?.url).toBe("ws://192.168.1.20:7878/api/events?token=tok-1");
    // And the host coming back is heard through that socket.
    FakeSocket.instances[0]?.open();
    expect(byId("reconnect-pill").hidden).toBe(true);
  });
});

describe("one socket at a time", () => {
  it("a wake during the backoff cancels the retry rather than opening a second socket", async () => {
    const { online, byId, fireTimers } = boot(() => okJson(null));
    await flush();
    const [first] = FakeSocket.instances;
    expect(first).toBeDefined();
    first?.open();
    // The network takes the socket: a retry is scheduled a second out.
    first?.drop();
    expect(byId("reconnect-pill").hidden).toBe(false);
    // The phone is looked at before the second is up. This opens a socket now.
    online();
    await flush();
    expect(FakeSocket.instances).toHaveLength(2);
    // The failure this prevents: the timer from the drop was never cleared,
    // so it fired anyway and the page had two live sockets, each painting.
    fireTimers();
    await flush();
    expect(FakeSocket.instances).toHaveLength(2);
    expect(FakeSocket.instances[1]?.closed).toBe(false);
  });
});

describe("the chat block under a finger", () => {
  /** The page on the Chat tab with one entry showing and the input focused. */
  async function typingIntoChat(): Promise<
    Booted & { socket: FakeSocket; input: HTMLInputElement }
  > {
    const booted = boot(() => okJson(null));
    await flush();
    const socket = FakeSocket.instances[0];
    if (!socket) throw new Error("no socket");
    socket.open();
    (document.querySelector('[data-tab="chat"]') as HTMLButtonElement).click();
    socket.frame("shared-chat", {
      league_id: "league-1",
      screen: "season",
      busy: false,
      entries: [
        {
          role: "user",
          text: "who do I start?",
          at_ms: Date.now() - 5_000,
          device: { name: "Rob's iPhone", kind: "phone" },
          cost_usd: null,
        },
      ],
    });
    const input = booted.byId("chat-input") as HTMLInputElement;
    input.focus();
    return { ...booted, socket, input };
  }

  it("a repaint that changes nothing in the thread leaves the list and the focus alone", async () => {
    const { socket, input, byId } = await typingIntoChat();
    const list = byId("chat-list");
    const entry = list.firstElementChild;
    expect(entry?.textContent).toContain("who do I start?");
    expect(document.activeElement).toBe(input);
    // The failure this prevents: every clock tick moved the block and rebuilt
    // the list, and on iOS that took the keyboard down mid-word. A health
    // frame is any repaint with the thread unchanged.
    socket.frame("poll-health", { consecutive_failures: 0 });
    expect(list.firstElementChild).toBe(entry);
    expect(byId("chat-block").parentElement?.id).toBe("tab-chat");
    expect(document.activeElement).toBe(input);
    // A new thread does rebuild it.
    socket.frame("shared-chat", {
      league_id: "league-1",
      screen: "season",
      busy: false,
      entries: [],
    });
    expect(list.firstElementChild).not.toBe(entry);
    expect(list.textContent).toContain("Nothing asked yet.");
  });

  it("empties the box once the host has taken the question", async () => {
    const { input, byId } = await typingIntoChat();
    input.value = "is Rob's trade fair?";
    byId("chat-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await flush();
    // The other half of the rule below: the question is held only until the
    // host has it. Without this, never clearing the box would pass too.
    expect(input.value).toBe("");
    expect(byId("chat-note").hidden).toBe(true);
  });

  it("a question the host did not take is put back with a note, not lost", async () => {
    const { input, byId, fetch } = await typingIntoChat();
    fetch.mockImplementation((path: string) =>
      path === "/api/chat" ? Promise.reject(new Error("the host is away")) : okJson(null),
    );
    input.value = "is Rob's trade fair?";
    byId("chat-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await flush();
    // The failure this prevents: the input was cleared before the request,
    // the request threw past the handler, and the question was gone with
    // nothing on screen saying so.
    expect(input.value).toBe("is Rob's trade fair?");
    expect(byId("chat-note").hidden).toBe(false);
    expect(byId("chat-note").textContent).toBe("The host did not answer.");
  });
});

describe("the screen during a draft", () => {
  const drafting = {
    draft: { status: "drafting", current_pick: 12, current_round: 2, on_clock_slot: 3 },
    recommendations: [],
    recent_picks: [],
    my_roster: { players: [], open_starters: [] },
    data_health: { board_size: 300 },
  };

  it("is held awake while a draft is live and connected, and let go when the host drops", async () => {
    const wakeLock = fakeWakeLock();
    boot(() => okJson(null), { wakeLock });
    await flush();
    const socket = FakeSocket.instances[0];
    if (!socket) throw new Error("no socket");
    socket.open();
    // Connected, but nothing is being drafted: the phone may sleep.
    expect(wakeLock.request).not.toHaveBeenCalled();
    socket.frame("draft-updated", drafting);
    await flush();
    // The failure this prevents: the phone dimmed and locked mid-draft, and
    // the pick clock was behind a lock screen when it mattered.
    expect(wakeLock.request).toHaveBeenCalledWith("screen");
    expect(wakeLock.request).toHaveBeenCalledTimes(1);
    // A repaint with the draft still live does not ask twice.
    socket.frame("poll-health", { consecutive_failures: 0 });
    await flush();
    expect(wakeLock.request).toHaveBeenCalledTimes(1);
    // The host goes away: the lock is released rather than burning the
    // battery on a "Reconnecting" pill.
    socket.drop();
    await flush();
    expect(wakeLock.sentinels[0]?.released).toBe(true);
  });

  it("does nothing on a phone or address with no wake lock", async () => {
    // The plain http LAN address has no `navigator.wakeLock` at all; the
    // page must not throw its way out of render for want of one.
    const { byId } = boot(() => okJson(null));
    await flush();
    const socket = FakeSocket.instances[0];
    socket?.open();
    socket?.frame("draft-updated", drafting);
    expect(byId("clock-strip").textContent).toContain("Pick 12");
  });
});

describe("what the phone says about the host", () => {
  const board = {
    draft: { status: "drafting", current_pick: 12, current_round: 2, on_clock_slot: 3 },
    recommendations: [],
    recent_picks: [],
    my_roster: { players: [], open_starters: [] },
    data_health: { board_size: 300, poll_consecutive_failures: 0, poll_last_success_at: 1000 },
  };

  it("says the host's sync is off rather than 'sync healthy' over a frozen board", async () => {
    // The whole failure: the line read the failed-poll count, which is 0 both
    // on a host that is syncing and on one that is not polling at all, so a
    // phone printed "sync healthy" with no timestamp beside it while the pick
    // clock counted down over a board that had stopped moving.
    const { byId } = boot(() => okJson(null));
    await flush();
    const socket = FakeSocket.instances[0];
    if (!socket) throw new Error("no socket");
    socket.open();
    socket.frame("draft-updated", board);
    socket.frame("hello", { server_now_ms: Date.now(), host_name: "Justin's Mac", polling: false });
    await flush();
    expect(byId("health").textContent).toContain("The host's live sync is off");
    expect(byId("health").textContent).toContain("300 players on the board");
    expect(byId("health").textContent).not.toContain("healthy");
    // The host turns live sync back on: the next heartbeat says so.
    socket.frame("pong", { server_now_ms: Date.now(), host_name: "Justin's Mac", polling: true });
    await flush();
    expect(byId("health").textContent).toContain("Syncing");
    expect(byId("health").textContent).not.toContain("off");
  });

  it("takes the host's new name off the socket rather than the pairing it did once", async () => {
    // The failure this prevents: the name was written down when the phone
    // paired, so renaming the Mac under "Your name in shared chat" left every
    // paired phone showing the old one until it paired again.
    const { byId, stored } = boot(() => okJson(null), {
      saved: { "da.companion.token": "tok-1", "da.companion.host": "Justin's Mac" },
    });
    await flush();
    const socket = FakeSocket.instances[0];
    if (!socket) throw new Error("no socket");
    socket.open();
    socket.frame("hello", { server_now_ms: Date.now(), host_name: "The Big Board", polling: true });
    await flush();
    expect(stored("da.companion.host")).toBe("The Big Board");
    // And that is the name the pairing screen offers if the token is dropped.
    socket.frame("revoked", {});
    await flush();
    expect(byId("pair-host").textContent).toBe("Hosted by The Big Board");
  });
});
