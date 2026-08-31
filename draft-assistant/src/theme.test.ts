// Grade item D6. Theme resolution is one of the two halves of the app's
// persistence layer, and the half nobody had tested directly: what happens
// when the OS setting flips, when the user overrides it, when the stored word
// is not one of ours, and when the webview has no `matchMedia` at all.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyTheme,
  cycleThemePreference,
  resetThemePreference,
  resolveTheme,
  systemTheme,
  useAppliedTheme,
} from "./theme";

/** A `matchMedia` whose answer this test controls, and which reports whether
 * anything is still listening to it. */
function fakeMedia(dark: boolean) {
  const handlers = new Set<(event: MediaQueryListEvent) => void>();
  let removals = 0;
  const list = {
    matches: dark,
    addEventListener: (_type: string, handler: (event: MediaQueryListEvent) => void) => {
      handlers.add(handler);
    },
    removeEventListener: (_type: string, handler: (event: MediaQueryListEvent) => void) => {
      removals += 1;
      handlers.delete(handler);
    },
  };
  vi.stubGlobal("matchMedia", () => list as unknown as MediaQueryList);
  return {
    /** The OS setting changes under the app's feet. */
    flip: (nowDark: boolean) => {
      list.matches = nowDark;
      act(() => {
        for (const handler of [...handlers]) {
          handler({ matches: nowDark } as MediaQueryListEvent);
        }
      });
    },
    listening: () => handlers.size,
    removals: () => removals,
  };
}

function fakeStorage(initial: Record<string, string> = {}): Map<string, string> {
  const store = new Map(Object.entries(initial));
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
  return store;
}

beforeEach(() => {
  fakeStorage();
  fakeMedia(false);
  resetThemePreference();
  delete document.documentElement.dataset.theme;
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetThemePreference();
});

describe("systemTheme", () => {
  it("reads the OS preference, and assumes light without one", () => {
    fakeMedia(true);
    expect(systemTheme()).toBe("dark");
    fakeMedia(false);
    expect(systemTheme()).toBe("light");
    // Some webviews have no `matchMedia` at all; that is not a crash, it is a
    // light-mode window.
    vi.stubGlobal("matchMedia", undefined);
    expect(systemTheme()).toBe("light");
  });
});

describe("resolveTheme", () => {
  it("passes an explicit choice through and defers only for 'system'", () => {
    fakeMedia(true);
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
    expect(resolveTheme("system")).toBe("dark");
  });
});

describe("applyTheme", () => {
  it("paints the page by setting the attribute the stylesheets key on", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("cycleThemePreference", () => {
  it("steps system -> light -> dark -> system and remembers each stop", () => {
    const store = fakeStorage();
    const { result } = renderHook(() => useAppliedTheme());
    expect(result.current.preference).toBe("system");

    act(() => cycleThemePreference());
    expect(result.current.preference).toBe("light");
    expect(store.get("da.theme")).toBe("light");

    act(() => cycleThemePreference());
    expect(result.current.preference).toBe("dark");
    expect(store.get("da.theme")).toBe("dark");

    act(() => cycleThemePreference());
    expect(result.current.preference).toBe("system");
    expect(store.get("da.theme")).toBe("system");
  });

  it("still applies for this session when the write is refused", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
      removeItem: () => undefined,
    });
    resetThemePreference();
    const { result } = renderHook(() => useAppliedTheme());
    act(() => cycleThemePreference());
    expect(result.current.preference).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("useAppliedTheme", () => {
  it("starts from what was stored, ignoring anything it does not recognise", () => {
    fakeStorage({ "da.theme": "solarized" });
    resetThemePreference();
    const { result } = renderHook(() => useAppliedTheme());
    expect(result.current.preference).toBe("system");

    fakeStorage({ "da.theme": "dark" });
    resetThemePreference();
    const stored = renderHook(() => useAppliedTheme());
    expect(stored.result.current.preference).toBe("dark");
    expect(stored.result.current.theme).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("follows the OS while the preference is 'system'", () => {
    const media = fakeMedia(false);
    const { result } = renderHook(() => useAppliedTheme());
    expect(result.current.theme).toBe("light");
    expect(media.listening()).toBe(1);

    media.flip(true);
    expect(document.documentElement.dataset.theme).toBe("dark");
    media.flip(false);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("stops following the OS once the user has overridden it", () => {
    const media = fakeMedia(false);
    renderHook(() => useAppliedTheme());
    expect(media.listening()).toBe(1);

    act(() => cycleThemePreference());
    // The explicit choice is in force and nothing is watching the OS any more:
    // a system flip must not undo what the user just picked.
    expect(media.listening()).toBe(0);
    expect(media.removals()).toBe(1);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("lets go of the OS listener when the app goes away", () => {
    const media = fakeMedia(true);
    const { unmount } = renderHook(() => useAppliedTheme());
    expect(media.listening()).toBe(1);
    unmount();
    expect(media.listening()).toBe(0);
  });

  it("works in a webview with no matchMedia to watch", () => {
    vi.stubGlobal("matchMedia", undefined);
    const { result, unmount } = renderHook(() => useAppliedTheme());
    expect(result.current.theme).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    // Nothing was subscribed, so tearing down has nothing to undo.
    expect(() => unmount()).not.toThrow();
  });
});
