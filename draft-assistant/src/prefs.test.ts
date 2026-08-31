import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { persisted, usePersisted } from "./persisted";
import {
  chimeOn,
  resetPrefs,
  setChime,
  setLineupView,
  setScreen,
  useChime,
  useLineupView,
  useScreen,
} from "./prefs";
import { cycleThemePreference, resetThemePreference, useAppliedTheme } from "./theme";

/** A storage that starts with whatever an older version of the app wrote. */
function storageHolding(entries: Record<string, string>): Map<string, string> {
  const saved = new Map(Object.entries(entries));
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => void saved.set(key, value),
    removeItem: (key: string) => void saved.delete(key),
  });
  forgetThisSession();
  return saved;
}

/** A storage that refuses to do anything, as a private window does. */
function lockedStorage(): void {
  vi.stubGlobal("localStorage", {
    getItem: () => {
      throw new Error("storage is not available");
    },
    setItem: () => {
      throw new Error("storage is not available");
    },
    removeItem: () => undefined,
  });
  forgetThisSession();
}

function forgetThisSession(): void {
  resetPrefs();
  resetThemePreference();
}

afterEach(() => {
  vi.unstubAllGlobals();
  forgetThisSession();
});

describe("preferences that were saved by an earlier version", () => {
  it("still apply, exactly as they were stored", () => {
    storageHolding({
      "da.chime": "off",
      "da.screen": "draft",
      "da.theme": "dark",
      "da.lineupView": "Scoreboard",
    });

    const { result } = renderHook(() => ({
      chime: useChime(),
      screen: useScreen(),
      lineup: useLineupView(),
      appearance: useAppliedTheme(),
    }));

    expect(result.current.chime).toBe(false);
    expect(result.current.screen).toBe("draft");
    expect(result.current.lineup).toBe("Scoreboard");
    expect(result.current.appearance.preference).toBe("dark");
    expect(result.current.appearance.theme).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("are written back in the same words, so the next version can read them", () => {
    const saved = storageHolding({});

    setChime(false);
    setScreen("draft");
    setLineupView("Scoreboard");
    // system -> light is the first step of the appearance cycle.
    cycleThemePreference();

    expect(Object.fromEntries(saved)).toEqual({
      "da.chime": "off",
      "da.screen": "draft",
      "da.lineupView": "Scoreboard",
      "da.theme": "light",
    });
  });

  it("fall back to the defaults when the stored text is not one of ours", () => {
    // Every preference, not just one: a value written by a future version, or
    // corrupted, must not be able to put the app into a state it cannot name.
    storageHolding({
      "da.screen": "nonsense",
      "da.theme": "aubergine",
      "da.chime": "maybe",
      "da.lineupView": "Grid",
    });

    const { result } = renderHook(() => ({
      screen: useScreen(),
      appearance: useAppliedTheme(),
      chime: useChime(),
      lineup: useLineupView(),
    }));

    expect(result.current.screen).toBe("season");
    expect(result.current.appearance.preference).toBe("system");
    expect(result.current.chime).toBe(true);
    expect(result.current.lineup).toBe("Table");
  });

  it("read back the defaults when those are what was stored", () => {
    // The other side of each guard: the stored word is one of ours and happens
    // to be the default, which must round-trip rather than being re-derived.
    storageHolding({ "da.chime": "on", "da.lineupView": "Table", "da.screen": "season" });

    const { result } = renderHook(() => ({
      screen: useScreen(),
      chime: useChime(),
      lineup: useLineupView(),
    }));

    expect(result.current.screen).toBe("season");
    expect(result.current.chime).toBe(true);
    expect(result.current.lineup).toBe("Table");
    expect(chimeOn()).toBe(true);
  });
});

describe("a preference when storage will not play", () => {
  it("falls back to its default rather than throwing", () => {
    lockedStorage();
    expect(chimeOn()).toBe(true);
    expect(renderHook(() => useScreen()).result.current).toBe("season");
  });

  it("still applies the change for the rest of the session", () => {
    lockedStorage();
    const { result } = renderHook(() => useChime());

    act(() => {
      setChime(false);
    });

    expect(result.current).toBe(false);
    expect(chimeOn()).toBe(false);
  });
});

describe("the persisted helper itself", () => {
  it("tells every reader when the value changes, and stops when they leave", () => {
    storageHolding({});
    const store = persisted<"yes" | "no">("da.test", (raw) => (raw === "no" ? "no" : "yes"), "yes");
    const heard = vi.fn();
    const stop = store.subscribe(heard);

    store.set("no");
    expect(heard).toHaveBeenCalledTimes(1);
    expect(store.get()).toBe("no");

    stop();
    store.set("yes");
    expect(heard).toHaveBeenCalledTimes(1);
  });

  it("re-reads storage after the session's choice is forgotten", () => {
    const saved = storageHolding({ "da.test": "no" });
    const store = persisted<"yes" | "no">("da.test", (raw) => (raw === "no" ? "no" : "yes"), "yes");

    expect(store.get()).toBe("no");
    store.set("yes");

    const { result } = renderHook(() => usePersisted(store));
    expect(result.current).toBe("yes");

    // What is on disk, changed from under the store.
    saved.set("da.test", "no");
    act(() => {
      store.reset();
    });
    expect(result.current).toBe("no");
  });
});
