// The companion phone page, booted for real under jsdom.
//
// `src-tauri/companion-static/*.js` are plain scripts served straight to a
// phone, so there is nothing to import: this gives the shipped files the real
// `index.html`, a `fetch` and a `WebSocket` of their own, and a window whose
// timers are held rather than run, so a test says when a backoff has elapsed
// and nothing fires between its lines otherwise. Shared by the boot tests and
// the install tests, which each ask the same page different questions.
//
// Each boot gets a `window` of its own over the shared jsdom `document`: the
// page hangs listeners on `window` that no test can take down again, and a
// second boot on the same window would wake the first page's closure too.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import { vi, type Mock } from "vitest";

const asset = (file: string): string =>
  readFileSync(resolve(`src-tauri/companion-static/${file}`), "utf8");
const page = asset("index.html");
const scripts = ["helpers.js", "clock.js", "pwa.js", "app.js"].map(asset);

export const TOKEN_KEY = "da.companion.token";
export const DEVICE_KEY = "da.companion.device";
export const DEVICE_ID_KEY = "da.companion.device-id";

/** A `WebSocket` the test opens, drops and feeds by hand. */
export class FakeSocket {
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

export const okJson = (body: unknown, status = 200) =>
  Promise.resolve({ ok: status >= 200 && status < 300, status, json: () => Promise.resolve(body) });

/** Let every pending promise settle; the page's reads are all microtasks. */
export async function flush(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

export type Fetch = (path: string, init?: RequestInit) => Promise<unknown>;

/** A booted page: its window's hooks, and the timers it set, to fire by hand. */
export interface Booted {
  fetch: Mock<Fetch>;
  /** The phone finds a network: what `window` hears as `online`. */
  online: () => void;
  /** Run every timeout the page has pending, as if the time had passed. */
  fireTimers: () => void;
  byId: (id: string) => HTMLElement;
  /** What the page has in storage, by key. */
  stored: (key: string) => string | null;
  /** The address the page last wrote into the bar, or null. */
  address: () => string | null;
}

/** What a phone's `navigator.wakeLock` hands back, and what it was asked. */
export interface FakeWakeLock {
  request: Mock<(kind: string) => Promise<FakeSentinel>>;
  sentinels: FakeSentinel[];
}
export interface FakeSentinel {
  released: boolean;
  release: () => Promise<void>;
  /** The browser let go of it (the screen went dark): what the page hears. */
  releasedByBrowser: () => void;
  addEventListener: (type: string, fn: () => void) => void;
}
export function fakeWakeLock(): FakeWakeLock {
  const sentinels: FakeSentinel[] = [];
  const request = vi.fn<(kind: string) => Promise<FakeSentinel>>(() => {
    const listeners: (() => void)[] = [];
    const sentinel: FakeSentinel = {
      released: false,
      release: () => {
        sentinel.released = true;
        return Promise.resolve();
      },
      releasedByBrowser: () => {
        sentinel.released = true;
        for (const fn of listeners) fn();
      },
      addEventListener: (type, fn) => {
        if (type === "release") listeners.push(fn);
      },
    };
    sentinels.push(sentinel);
    return Promise.resolve(sentinel);
  });
  return { request, sentinels };
}

export interface BootOptions {
  wakeLock?: FakeWakeLock;
  /** What storage holds when the page opens. Defaults to a saved token. */
  saved?: Record<string, string>;
  /** The query on the page's address, `?device=...` say. */
  search?: string;
  /** Present on iOS Safari and nowhere else; the page reads its presence. */
  standalone?: boolean;
  /** Whether the window has an `AbortController`, which the deadline needs. */
  abortable?: boolean;
}

/** The real page, booted over the given fetch. */
export function boot(fetch: Fetch, options: BootOptions = {}): Booted {
  document.body.innerHTML = page.slice(
    page.indexOf('<div id="companion-root">'),
    page.indexOf("<script"),
  );
  document.head.innerHTML = "";
  FakeSocket.instances = [];
  const pending = new Map<number, () => void>();
  let nextTimer = 1;
  const fetchSpy: Mock<Fetch> = vi.fn(fetch);
  const events = new EventTarget();
  const saved = new Map<string, string>(Object.entries(options.saved ?? { [TOKEN_KEY]: "tok-1" }));
  let address: string | null = null;
  // The window the page sees: the address bar, the storage, its timers and
  // its events.
  const window = {
    location: {
      protocol: "http:",
      host: "192.168.1.20:7878",
      pathname: "/",
      search: options.search ?? "",
      hash: "",
    },
    history: {
      replaceState: (_state: unknown, _title: string, url: string) => {
        address = url;
      },
    },
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
    ...(options.abortable ? { AbortController } : {}),
  };
  const sandbox = {
    window,
    document,
    navigator: {
      userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)",
      ...(options.wakeLock ? { wakeLock: options.wakeLock } : {}),
      ...(options.standalone === undefined ? {} : { standalone: options.standalone }),
    },
    fetch: fetchSpy,
    WebSocket: FakeSocket,
    URLSearchParams,
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
    stored: (key) => saved.get(key) ?? null,
    address: () => address,
  };
}

/** Make jsdom's document report itself hidden or visible. */
export function setVisibility(state: "visible" | "hidden"): void {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
}
