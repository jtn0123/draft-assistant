// The Now tab's roster and the signals line above Recommended: what the
// phone has drafted, in pick order, the starting slots still open, and the
// one line about a run or a tier about to go, each repainted only when its
// data moves rather than on every clock second.

import { afterEach, expect, it } from "vitest";
import { boot, FakeSocket, flush, okJson, type Booted } from "./test/companionPageHarness";

interface RosterPlayer {
  player_id: string;
  name: string;
  position: string;
  team: string;
  pick_no: number;
  round: number;
  is_keeper: boolean;
}
interface TierAlert {
  position: string;
  tier: number;
  players_left: number;
}
interface PositionRun {
  position: string;
  count: number;
  window: number;
}
interface Signals {
  tier_alerts?: TierAlert[];
  position_run?: PositionRun | null;
}

const drafted = (pick_no: number, position: string, extra: Partial<RosterPlayer> = {}) => ({
  player_id: `p${pick_no}`,
  name: `${position} ${pick_no}`,
  position,
  team: "ATL",
  pick_no,
  round: Math.ceil(pick_no / 10),
  is_keeper: false,
  ...extra,
});

const view = (
  players: RosterPlayer[],
  open_starters: [string, number][] = [],
  signals: Signals = {},
) => ({
  draft: { status: "drafting", current_pick: 4, current_round: 1, on_clock_slot: 1, my_slot: 2 },
  available: [],
  recommendations: [],
  recent_picks: [],
  my_roster: { players, open_starters },
  tier_alerts: signals.tier_alerts ?? [],
  position_run: signals.position_run ?? null,
  data_health: { board_size: 0 },
});

type Frame = ReturnType<typeof view> | null;

/** A paired page on the Now tab, fed one draft view over the socket. */
async function nowTab(frame: Frame): Promise<{ page: Booted; feed: (frame: Frame) => void }> {
  const page = boot(() => okJson(null), { saved: { "da.companion.token": "tok-1" } });
  await flush();
  FakeSocket.instances[0]?.open();
  const feed = (next: Frame) => FakeSocket.instances[0]?.frame("draft-updated", next);
  feed(frame);
  return { page, feed };
}
const rows = (page: Booted) => [...page.byId("roster").querySelectorAll(".roster-row")];
const text = (row: Element, selector: string) => row.querySelector(selector)?.textContent ?? null;

afterEach(() => {
  document.body.innerHTML = "";
});

// ---- the roster ------------------------------------------------------------

it("lists what has been drafted in pick order, with the pick label and a keeper tag", async () => {
  const players = [
    drafted(22, "WR"),
    drafted(2, "RB", { name: "Bijan Robinson" }),
    drafted(81, "TE", { is_keeper: true }),
  ];
  const { page } = await nowTab(view(players, [["QB", 1]]));
  const shown = rows(page);
  expect(shown.map((row) => text(row, ".pick-no"))).toEqual(["1.2", "3.22", "9.81"]);
  expect(shown.map((row) => text(row, ".name"))).toEqual(["Bijan Robinson", "WR 22", "TE 81"]);
  expect(shown[0].querySelector(".pos")?.className).toBe("pos pos-rb");
  expect(text(shown[0], ".muted")).toBe("ATL");
  expect(shown.map((row) => text(row, ".kind"))).toEqual([null, null, "keeper"]);
  expect(page.byId("roster").querySelector("p")).toBeNull();
});

it("says which starting slots are still open, and nothing when they are all filled", async () => {
  const open: [string, number][] = [
    ["QB", 1],
    ["RB", 2],
    ["FLEX", 0],
  ];
  const { page, feed } = await nowTab(view([drafted(2, "RB")], open));
  expect(page.byId("roster-open").textContent).toBe("Still to fill: 1 QB, 2 RB");
  feed(view([drafted(2, "RB")], []));
  expect(page.byId("roster-open").textContent).toBe("");
});

it("says so when nothing has been drafted, or there is no draft at all", async () => {
  const { page, feed } = await nowTab(view([], [["QB", 1]]));
  const empty = page.byId("roster").querySelector("p");
  expect(empty?.className).toBe("muted");
  expect(empty?.textContent).toBe("Nothing drafted yet.");
  expect(rows(page)).toHaveLength(0);
  feed(null);
  expect(page.byId("roster").textContent).toBe("Nothing drafted yet.");
  expect(page.byId("roster-open").textContent).toBe("");
});

it("keeps the same row nodes when the roster has not changed, and swaps them when it has", async () => {
  const { page, feed } = await nowTab(view([drafted(2, "RB"), drafted(19, "WR")]));
  const before = rows(page);
  expect(before).toHaveLength(2);
  feed(view([drafted(2, "RB"), drafted(19, "WR")]));
  const after = rows(page);
  expect(after).toHaveLength(2);
  after.forEach((row, i) => expect(row).toBe(before[i]));
  feed(view([drafted(2, "RB"), drafted(19, "WR"), drafted(22, "TE")]));
  const grown = rows(page);
  expect(grown).toHaveLength(3);
  expect(grown[0]).not.toBe(before[0]);
});

// ---- the signals line ------------------------------------------------------

it("names a tier about to run out, and stays hidden when every tier has plenty", async () => {
  const alerts = [
    { position: "QB", tier: 2, players_left: 20 },
    { position: "TE", tier: 3, players_left: 1 },
    { position: "RB", tier: 5, players_left: 3 },
  ];
  const { page, feed } = await nowTab(view([], [], { tier_alerts: alerts }));
  const signals = page.byId("signals");
  expect(signals.hidden).toBe(false);
  expect(signals.textContent).toBe("TE tier 3: 1 left · RB tier 5: 3 left");
  feed(view([], [], { tier_alerts: [{ position: "QB", tier: 2, players_left: 20 }] }));
  expect(signals.hidden).toBe(true);
  expect(signals.textContent).toBe("");
  feed(null);
  expect(signals.hidden).toBe(true);
});

it("words a position run the way the desktop does, ahead of the tier alerts", async () => {
  const signals = {
    position_run: { position: "RB", count: 4, window: 6 },
    tier_alerts: [{ position: "TE", tier: 3, players_left: 2 }],
  };
  const { page, feed } = await nowTab(view([], [], signals));
  expect(page.byId("signals").hidden).toBe(false);
  expect(page.byId("signals").textContent).toBe(
    "RB run in progress: 4 of the last 6 · TE tier 3: 2 left",
  );
  feed(view([], [], { position_run: { position: "WR", count: 3, window: 5 } }));
  expect(page.byId("signals").textContent).toBe("WR run in progress: 3 of the last 5");
});
