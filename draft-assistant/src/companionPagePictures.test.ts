// The faces on the phone page: a team logo the moment a row is drawn, the
// player's photo from the host when it lands, and one ask per player for the
// life of the page. `src-tauri/companion-static/pictures.js` is a plain
// script, so the page is booted for real under the harness for the rows, and
// the loader is run on its own for what the harness window cannot reach.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";
import {
  boot,
  FakeSocket,
  flush,
  okJson,
  type Booted,
  type Fetch,
} from "./test/companionPageHarness";

interface Player {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
}
interface Pictures {
  avatar(this: void, player: unknown): HTMLElement;
}
type CreatePictures = (win: object, getToken: () => string | null) => Pictures;

const LOGO = (team: string) => `https://sleepercdn.com/images/team_logos/nfl/${team}.png`;
const PNG_DATA_URL = "data:image/png;base64,cG5n";
const jsdomWindow = () => {
  const view = document.defaultView;
  if (!view) throw new Error("no jsdom window");
  return view;
};

/** What the host answers for one player's headshot. */
type Photo = "png" | "none" | "offline";
const headshotResponse = (photo: Photo): Promise<unknown> => {
  if (photo === "offline") return Promise.reject(new Error("Failed to fetch"));
  if (photo === "none") return okJson({ error: "no picture" }, 404);
  // jsdom's FileReader reads only its own Blob, and the page reads whatever
  // the host's bytes were wrapped in.
  const blob = new (jsdomWindow().Blob)(["png"], { type: "image/png" });
  return Promise.resolve({ ok: true, status: 200, blob: () => Promise.resolve(blob) });
};
/** A host fetch that serves pictures by player id and shrugs at the rest. */
const host =
  (photos: Record<string, Photo>): Fetch =>
  (path) => {
    const match = /^\/api\/headshot\/(.+)$/.exec(path);
    if (match) return headshotResponse(photos[match[1]] ?? "none");
    return okJson(null);
  };
const headshotCalls = (page: Booted) =>
  page.fetch.mock.calls.filter(([path]) => path.startsWith("/api/headshot/"));

const player = (player_id: string, team: string | null, position = "WR"): Player => ({
  player_id,
  name: `Player ${player_id}`,
  position,
  team,
});
const view = (recommendations: Player[], picks: Player[]) => ({
  draft: {
    status: "drafting",
    teams: 12,
    rounds: 15,
    current_pick: 5,
    current_round: 1,
    on_clock_slot: 5,
    my_slot: 5,
    is_my_pick: true,
    picks_until_mine: 0,
    total_picks_made: 4,
    keeper_picks: [],
    is_auction: false,
  },
  recommendations: recommendations.map((rec) => ({
    ...rec,
    mode: "balanced",
    tier: 1,
    adp: 3.5,
    survival_next: 0.5,
    reasons: [],
  })),
  recent_picks: picks.map((pick, i) => ({
    ...pick,
    pick_no: i + 1,
    round: 1,
    slot: i + 1,
    slot_name: `Manager ${i + 1}`,
  })),
  available: [],
  rosters: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: 0 },
});

/** A paired page fed one draft view: the Now tab draws the cards. */
async function paired(
  photos: Record<string, Photo>,
): Promise<{ page: Booted; feed: (recs: Player[], picks: Player[]) => void }> {
  const page = boot(host(photos));
  await flush();
  FakeSocket.instances[0]?.open();
  const feed = (recs: Player[], picks: Player[]) =>
    FakeSocket.instances[0]?.frame("draft-updated", view(recs, picks));
  return { page, feed };
}
const showPicks = () => document.querySelector<HTMLButtonElement>('[data-tab="picks"]')?.click();
const cardImg = (page: Booted) => page.byId("recs").querySelector<HTMLImageElement>(".avatar img");
const rowImg = (page: Booted) => page.byId("picks").querySelector<HTMLImageElement>(".avatar img");
/** jsdom's FileReader answers through a chain of `setImmediate`s, so a
 *  photo lands only after a few turns of the event loop. */
const settle = async () => {
  for (let i = 0; i < 5; i += 1) {
    await flush();
    await new Promise((done) => setImmediate(done));
  }
  await flush();
};

/** The loader alone, over a window the test owns. */
const loadPictures = (doc: unknown = document): CreatePictures => {
  const window: { Companion?: { createPictures: CreatePictures } } = {};
  runInNewContext(readFileSync(resolve("src-tauri/companion-static/pictures.js"), "utf8"), {
    window,
    document: doc,
  });
  if (!window.Companion) throw new Error("pictures.js added nothing to window.Companion");
  return window.Companion.createPictures;
};

afterEach(() => {
  document.body.innerHTML = "";
});

describe("on the page", () => {
  it("draws the team logo on a recommendation card and a recent-pick row", async () => {
    const { page, feed } = await paired({});
    feed([player("11", "KC")], [player("22", "buf", "RB")]);
    const card = cardImg(page);
    expect(card?.getAttribute("src")).toBe(LOGO("kc"));
    expect(card?.parentElement?.className).toBe("avatar");
    expect(card?.getAttribute("alt")).toBe("");
    expect(card?.getAttribute("width")).toBe("28");
    expect(card?.getAttribute("height")).toBe("28");
    expect(card?.getAttribute("loading")).toBe("lazy");
    expect(card?.getAttribute("decoding")).toBe("async");
    showPicks();
    expect(rowImg(page)?.getAttribute("src")).toBe(LOGO("buf"));
  });

  it("asks the host once per player, with the token, and swaps the photo in", async () => {
    const { page, feed } = await paired({ "11": "png" });
    feed([player("11", "KC")], [player("11", "KC")]);
    showPicks();
    const calls = headshotCalls(page);
    expect(calls).toHaveLength(1);
    const [path, init] = calls[0];
    expect(path).toBe("/api/headshot/11");
    expect((init?.headers as Record<string, string>).Authorization).toBe("Bearer tok-1");
    await settle();
    expect(rowImg(page)?.getAttribute("src")).toBe(PNG_DATA_URL);
    expect(rowImg(page)?.parentElement?.className).toBe("avatar");
    // Back on the Now tab the card is drawn again: the photo comes from the
    // page's memory, and the host is not asked a second time.
    document.querySelector<HTMLButtonElement>('[data-tab="now"]')?.click();
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("kc"));
    await flush();
    expect(cardImg(page)?.getAttribute("src")).toBe(PNG_DATA_URL);
    expect(headshotCalls(page)).toHaveLength(1);
  });

  it("remembers a 404 as no photo: the logo stays and nobody asks again", async () => {
    const { page, feed } = await paired({ "11": "none" });
    feed([player("11", "KC")], []);
    await settle();
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("kc"));
    feed([player("11", "KC")], []);
    await settle();
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("kc"));
    expect(headshotCalls(page)).toHaveLength(1);
  });

  it("does not remember a failed request: the next render asks again", async () => {
    const { page, feed } = await paired({ "11": "offline" });
    feed([player("11", "KC")], []);
    await settle();
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("kc"));
    feed([player("11", "KC")], []);
    await settle();
    expect(headshotCalls(page)).toHaveLength(2);
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("kc"));
  });

  it("gives a defence its own logo and asks the host for nothing", async () => {
    const { page, feed } = await paired({ JAX: "png" });
    feed([player("JAX", null, "DEF")], []);
    await settle();
    expect(cardImg(page)?.getAttribute("src")).toBe(LOGO("jax"));
    expect(headshotCalls(page)).toHaveLength(0);
  });
});

describe("the loader alone", () => {
  const win = () => ({
    fetch: vi.fn<Fetch>(() => headshotResponse("png")),
    FileReader: jsdomWindow().FileReader,
  });
  const err = (img: Element | null) => img?.dispatchEvent(new Event("error"));

  it("asks for nothing without a token, and still draws the logo", () => {
    const owner = win();
    const slot = loadPictures()(owner, () => null).avatar(player("11", "KC"));
    expect(slot.querySelector("img")?.getAttribute("src")).toBe(LOGO("kc"));
    expect(owner.fetch).not.toHaveBeenCalled();
  });

  it("reads the token at request time, so pairing after boot starts the photos", async () => {
    const owner = win();
    let token: string | null = null;
    const pictures = loadPictures()(owner, () => token);
    pictures.avatar(player("11", "KC"));
    token = "tok-2";
    const slot = pictures.avatar(player("11", "KC"));
    await settle();
    expect(slot.querySelector("img")?.getAttribute("src")).toBe(PNG_DATA_URL);
    const calls = owner.fetch.mock.calls as [string, RequestInit | undefined][];
    expect(calls).toHaveLength(1);
    expect((calls[0][1]?.headers as Record<string, string>).Authorization).toBe("Bearer tok-2");
  });

  it("goes back to the logo when the photo cannot draw, and hides a logo that cannot", async () => {
    const owner = win();
    const slot = loadPictures()(owner, () => "tok-1").avatar(player("11", "KC"));
    const img = slot.querySelector("img");
    await settle();
    expect(img?.getAttribute("src")).toBe(PNG_DATA_URL);
    err(img);
    expect(img?.getAttribute("src")).toBe(LOGO("kc"));
    expect(slot.className).toBe("avatar");
    err(img);
    expect(img?.hasAttribute("src")).toBe(false);
    expect(slot.className).toBe("avatar avatar-blank");
  });

  it("leaves an empty slot for a player with neither photo nor team, and never throws", () => {
    const pictures = loadPictures()(win(), () => "tok-1");
    for (const subject of [player("", null), null, undefined, 7, "QB"]) {
      const slot = pictures.avatar(subject);
      expect(slot.className).toBe("avatar avatar-blank");
      expect(slot.querySelector("img")).toBeNull();
    }
  });

  it("borrows the document's reader when the window has none", async () => {
    const owner: { fetch: Mock<Fetch>; FileReader?: unknown } = win();
    delete owner.FileReader;
    const slot = loadPictures()(owner, () => "tok-1").avatar(player("11", "KC"));
    await settle();
    expect(slot.querySelector("img")?.getAttribute("src")).toBe(PNG_DATA_URL);
    expect(owner.fetch).toHaveBeenCalledTimes(1);
  });

  it("shows no photo with no reader anywhere, and keeps the logo", async () => {
    const owner: { fetch: Mock<Fetch>; FileReader?: unknown } = win();
    delete owner.FileReader;
    const bare = { createElement: (tag: string) => document.createElement(tag), defaultView: null };
    const slot = loadPictures(bare)(owner, () => "tok-1").avatar(player("11", "KC"));
    await settle();
    expect(slot.querySelector("img")?.getAttribute("src")).toBe(LOGO("kc"));
    expect(slot.className).toBe("avatar");
    expect(owner.fetch).toHaveBeenCalledTimes(1);
  });
});
