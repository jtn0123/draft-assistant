import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";

/**
 * The companion phone page's DOM and socket half, booted for real.
 *
 * `companionPage.test.ts` runs the shipped files against a document that owns
 * no page, which is the early return the bootstrap takes; nothing there ever
 * reaches `connect()` or the painters. This file gives the same files the real
 * `index.html` under jsdom, a `fetch` and a `WebSocket` of its own, and asks
 * about the failures only the live half can have: a host that is away when
 * the page wakes, a retry timer nobody cancelled, a chat list rebuilt under
 * the finger typing into it.
 *
 * Each boot gets a `window` of its own, the way `companionPage.test.ts` runs
 * the files in a bare context, over the shared jsdom `document`: the page
 * hangs listeners on `window` that no test can take down again, and a second
 * boot on the same window would wake the first page's closure too.
 */

const asset = (file: string): string =>
  readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8");
const page = asset("index.html");
const scripts = ["helpers.js", "clock.js", "app.js"].map(asset);
const TOKEN_KEY = "da.companion.token";

/** A `WebSocket` the test opens, drops and feeds by hand. */
class FakeSocket {
  static instances: FakeSocket[] = [];
  readyState = 0;
  closed = false;
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: ((event: { code: number }) => void) | null = null;
  constructor(public url: string) {
    FakeSocket.instances.push(this);
  }
  send(): void {}
  close(): void {
    this.closed = true;
    this.readyState = 3;
  }
  open(): void {
    this.readyState = 1;
    this.onopen?.({});
  }
  /** The network took it: what the page sees is `onclose` with no revoke. */
  drop(code = 1006): void {
    this.readyState = 3;
    this.onclose?.({ code });
  }
  frame(type: string, payload: unknown): void {
    this.onmessage?.({ data: JSON.stringify({ type, payload }) });
  }
}

const okJson = (body: unknown) =>
  Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) });

/** Let every pending promise settle; the page's reads are all microtasks. */
async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

/** A booted page: its window's hooks, and the timers it set, to fire by hand. */
type Fetch = (path: string) => Promise<unknown>;
interface Booted {
  fetch: Mock<Fetch>;
  /** The phone finds a network: what `window` hears as `online`. */
  online: () => void;
  /** Run every timeout the page has pending, as if the time had passed. */
  fireTimers: () => void;
  byId: (id: string) => HTMLElement;
}

/** The real page, booted with a token already saved, over the given fetch. */
function boot(fetch: Fetch): Booted {
  document.body.innerHTML = page.slice(
    page.indexOf('<div id="companion-root">'),
    page.indexOf("<script"),
  );
  FakeSocket.instances = [];
  const pending = new Map<number, () => void>();
  let nextTimer = 1;
  const fetchSpy: Mock<Fetch> = vi.fn(fetch);
  const events = new EventTarget();
  const saved = new Map<string, string>([[TOKEN_KEY, "tok-1"]]);
  // The window the page sees: the address bar, the storage, its timers and
  // its events. The timers are held rather than run, so a test says when the
  // backoff has elapsed and nothing fires between its lines otherwise.
  const window = {
    location: { protocol: "http:", host: "192.168.1.20:7878" },
    localStorage: {
      getItem: (key: string) => saved.get(key) ?? null,
      setItem: (key: string, value: string) => void saved.set(key, value),
      removeItem: (key: string) => void saved.delete(key),
    },
    setTimeout: (fn: () => void) => {
      const id = nextTimer;
      nextTimer += 1;
      pending.set(id, fn);
      return id;
    },
    clearTimeout: (id: number) => void pending.delete(id),
    setInterval: () => 0,
    clearInterval: () => undefined,
    addEventListener: (type: string, fn: () => void) => events.addEventListener(type, fn),
  };
  const sandbox = {
    window,
    document,
    navigator: { userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)" },
    fetch: fetchSpy,
    WebSocket: FakeSocket,
  };
  const context = createContext(sandbox);
  for (const script of scripts) runInContext(script, context);
  if (document.readyState === "loading") document.dispatchEvent(new Event("DOMContentLoaded"));
  return {
    fetch: fetchSpy,
    online: () => void events.dispatchEvent(new Event("online")),
    fireTimers: () => {
      const due = [...pending.values()];
      pending.clear();
      for (const fn of due) fn();
    },
    byId: (id) => {
      const node = document.getElementById(id);
      if (!node) throw new Error(`no #${id} on the page`);
      return node;
    },
  };
}

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
