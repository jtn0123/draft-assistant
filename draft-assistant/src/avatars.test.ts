// Grade item D6. The other half of the persistence layer: the Headshots /
// Team logos preference, and the per-session cache that is supposed to ask the
// backend for each face exactly once.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  headshot: vi.fn(),
  avatar: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks }));

import {
  avatarMode,
  headshotSrc,
  resetAvatarCache,
  setAvatarMode,
  teamAvatarSrc,
  useAvatarMode,
} from "./avatars";

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
  vi.clearAllMocks();
  fakeStorage();
  resetAvatarCache();
  setAvatarMode("headshots");
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("the picture preference", () => {
  it("remembers a switch to team logos", () => {
    const store = fakeStorage();
    setAvatarMode("logos");
    expect(avatarMode()).toBe("logos");
    expect(store.get("da.avatars")).toBe("logos");
  });

  it("comes back as logos in the next session", async () => {
    fakeStorage({ "da.avatars": "logos" });
    vi.resetModules();
    const reloaded = await import("./avatars");
    expect(reloaded.avatarMode()).toBe("logos");
  });

  it("falls back to headshots for a word it does not recognise", async () => {
    fakeStorage({ "da.avatars": "cartoons" });
    vi.resetModules();
    const reloaded = await import("./avatars");
    expect(reloaded.avatarMode()).toBe("headshots");
  });

  it("still applies for this session when storage refuses the write", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("SecurityError");
      },
      removeItem: () => undefined,
    });
    setAvatarMode("logos");
    expect(avatarMode()).toBe("logos");
  });

  it("re-renders everything reading it when the choice changes", () => {
    const { result } = renderHook(() => useAvatarMode());
    expect(result.current).toBe("headshots");
    act(() => setAvatarMode("logos"));
    expect(result.current).toBe("logos");
  });

  it("stops notifying a component that has gone away", () => {
    const { result, unmount } = renderHook(() => useAvatarMode());
    unmount();
    act(() => setAvatarMode("logos"));
    // The unmounted reader keeps its last value rather than being updated.
    expect(result.current).toBe("headshots");
  });
});

describe("headshotSrc", () => {
  it("asks the backend once per player and hands out the same answer after", async () => {
    mocks.headshot.mockResolvedValue("asset://face.png");
    const first = headshotSrc("4046");
    const second = headshotSrc("4046");
    expect(first).toBe(second);
    expect(await first).toBe("asset://face.png");
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
    expect(mocks.headshot).toHaveBeenCalledWith("4046");

    await headshotSrc("6794");
    expect(mocks.headshot).toHaveBeenCalledTimes(2);
  });

  it("resolves to no picture rather than rejecting when the fetch fails", async () => {
    mocks.headshot.mockRejectedValue(new Error("offline"));
    await expect(headshotSrc("4046")).resolves.toBeNull();
  });

  it("asks again after the cache is dropped", async () => {
    mocks.headshot.mockResolvedValue(null);
    await headshotSrc("4046");
    resetAvatarCache();
    await headshotSrc("4046");
    expect(mocks.headshot).toHaveBeenCalledTimes(2);
  });
});

describe("teamAvatarSrc", () => {
  it("keeps the thumbnail and the zoomed copy as separate cache entries", async () => {
    mocks.avatar.mockImplementation((reference: string, full: boolean) =>
      Promise.resolve(full ? `${reference}-280` : reference),
    );

    expect(await teamAvatarSrc("abc123")).toBe("abc123");
    expect(await teamAvatarSrc("abc123")).toBe("abc123");
    expect(mocks.avatar).toHaveBeenCalledTimes(1);
    expect(mocks.avatar).toHaveBeenCalledWith("abc123", false);

    // The zoomed view wants the 280px copy: same reference, different picture,
    // so it must not be served the thumbnail already in the cache.
    expect(await teamAvatarSrc("abc123", true)).toBe("abc123-280");
    expect(mocks.avatar).toHaveBeenCalledTimes(2);
    expect(mocks.avatar).toHaveBeenLastCalledWith("abc123", true);

    expect(await teamAvatarSrc("abc123", true)).toBe("abc123-280");
    expect(mocks.avatar).toHaveBeenCalledTimes(2);
  });

  it("resolves to no picture rather than rejecting when the fetch fails", async () => {
    mocks.avatar.mockRejectedValue(new Error("offline"));
    await expect(teamAvatarSrc("abc123")).resolves.toBeNull();
    await expect(teamAvatarSrc("abc123", true)).resolves.toBeNull();
  });
});
