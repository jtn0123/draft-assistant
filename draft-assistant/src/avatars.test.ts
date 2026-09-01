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

  it("remembers that Sleeper has no photo, and does not ask twice", async () => {
    // A resolved null is a real answer: the player has no picture at all.
    mocks.headshot.mockResolvedValue(null);
    expect(await headshotSrc("4046")).toBeNull();
    expect(await headshotSrc("4046")).toBeNull();
    expect(mocks.headshot).toHaveBeenCalledTimes(1);
  });

  it("does not remember a failed request, so the next render asks again", async () => {
    // One network blip used to blank that player's face for the whole
    // session, because the rejection was cached as if it were an answer.
    mocks.headshot.mockRejectedValueOnce(new Error("offline"));
    expect(await headshotSrc("2216")).toBeNull();
    expect(mocks.headshot).toHaveBeenCalledTimes(1);

    mocks.headshot.mockResolvedValue("asset://evans.png");
    expect(await headshotSrc("2216")).toBe("asset://evans.png");
    expect(mocks.headshot).toHaveBeenCalledTimes(2);

    // ...and the answer that finally arrived is cached like any other.
    expect(await headshotSrc("2216")).toBe("asset://evans.png");
    expect(mocks.headshot).toHaveBeenCalledTimes(2);
  });

  it("lets a reset outlive a failure that has not settled yet", async () => {
    // The rejection lands after the cache was cleared and re-filled; it must
    // not delete the entry that replaced it.
    let fail: (e: Error) => void = () => undefined;
    mocks.headshot.mockReturnValueOnce(
      new Promise<string | null>((_, reject) => {
        fail = reject;
      }),
    );
    const first = headshotSrc("2216");
    resetAvatarCache();
    mocks.headshot.mockResolvedValue("asset://evans.png");
    expect(await headshotSrc("2216")).toBe("asset://evans.png");

    fail(new Error("offline"));
    expect(await first).toBeNull();
    expect(await headshotSrc("2216")).toBe("asset://evans.png");
    expect(mocks.headshot).toHaveBeenCalledTimes(2);
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

  it("does not remember a failed request, but does remember a real absence", async () => {
    mocks.avatar.mockRejectedValueOnce(new Error("offline"));
    expect(await teamAvatarSrc("abc123")).toBeNull();
    mocks.avatar.mockResolvedValue(null);
    expect(await teamAvatarSrc("abc123")).toBeNull();
    expect(mocks.avatar).toHaveBeenCalledTimes(2);
    // The second answer was a resolved null — a manager with no picture — so
    // nothing asks a third time.
    expect(await teamAvatarSrc("abc123")).toBeNull();
    expect(mocks.avatar).toHaveBeenCalledTimes(2);
  });
});
