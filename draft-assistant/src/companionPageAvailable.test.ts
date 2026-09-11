// The best-available list on the phone's Picks tab: who is still on the
// board, narrowed by a row of position chips, cheap enough to be asked for
// on every clock tick.

import { afterEach, expect, it } from "vitest";
import { boot, FakeSocket, flush, okJson, type Booted } from "./test/companionPageHarness";

const FILTER_KEY = "da.companion.available-filter";

interface Player {
  player_id: string;
  name: string;
  position: string;
  team: string;
  bye_week: number | null;
  tier: number;
  position_rank: number;
  overall_rank: number;
  adp: number | null;
  injury_status: string | null;
  survival_next: number | null;
}

const player = (overall_rank: number, position: string, extra: Partial<Player> = {}): Player => ({
  player_id: `p${overall_rank}`,
  name: `${position} ${overall_rank}`,
  position,
  team: "NO",
  bye_week: 8,
  tier: 2,
  position_rank: 1,
  overall_rank,
  adp: overall_rank + 0.5,
  injury_status: null,
  survival_next: 0.5,
  ...extra,
});

// Thirteen players: one more than the list shows folded, no K on the board.
const board = (): Player[] => [
  player(1, "WR", { injury_status: "Q", survival_next: 0.123 }),
  player(2, "RB", { adp: null, bye_week: null }),
  player(3, "QB"),
  ...[4, 5, 6, 7, 8, 9, 10, 11, 12].map((rank) => player(rank, rank % 2 ? "RB" : "TE")),
  player(13, "DEF", { name: "Saints" }),
];

const view = (available: Player[]) => ({
  draft: { status: "drafting", current_pick: 4, current_round: 1, on_clock_slot: 1, my_slot: 2 },
  available,
  recommendations: [],
  recent_picks: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: available.length },
});

/** A paired page on the Picks tab, fed one draft view over the socket. */
async function picksTab(
  available: Player[],
  saved: Record<string, string> = {},
): Promise<{ page: Booted; feed: (available: Player[]) => void }> {
  const page = boot(() => okJson(null), { saved: { "da.companion.token": "tok-1", ...saved } });
  await flush();
  FakeSocket.instances[0]?.open();
  const feed = (players: Player[]) =>
    FakeSocket.instances[0]?.frame("draft-updated", view(players));
  feed(available);
  document.querySelector<HTMLButtonElement>('[data-tab="picks"]')?.click();
  return { page, feed };
}
const chips = (page: Booted) =>
  [...page.byId("available-filter").querySelectorAll("button")].map((b) => [
    b.textContent,
    b.getAttribute("aria-pressed"),
  ]);
const rows = (page: Booted) => [...page.byId("available").querySelectorAll("li")];

afterEach(() => {
  document.body.innerHTML = "";
});

it("offers a chip per position on the board, in draft order, with All pressed", async () => {
  const { page } = await picksTab(board());
  expect(chips(page)).toEqual([
    ["All", "true"],
    ["QB", "false"],
    ["RB", "false"],
    ["WR", "false"],
    ["TE", "false"],
    ["DEF", "false"],
  ]);
  const shown = rows(page);
  expect(shown).toHaveLength(12);
  expect(shown.map((row) => row.querySelector(".rank")?.textContent)).toEqual([
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "10",
    "11",
    "12",
  ]);
  expect(shown[0].querySelector(".name")?.textContent).toBe("WR 1");
  expect(shown[0].querySelector(".pos")?.className).toBe("pos pos-wr");
  expect(shown[0].querySelector(".injury")?.textContent).toBe("Q");
  expect(shown[0].querySelector(".facts")?.textContent).toBe(
    "Tier 2 · ADP 1.5 · Bye 8 · 12% next turn",
  );
  expect(shown[1].querySelector(".injury")).toBeNull();
  expect(shown[1].querySelector(".facts")?.textContent).toBe("Tier 2 · ADP - · 50% next turn");
});

it("narrows to one position on a tap and remembers the chip across a reload", async () => {
  const { page } = await picksTab(board());
  const rb = [...page.byId("available-filter").querySelectorAll("button")].find(
    (b) => b.textContent === "RB",
  );
  rb?.click();
  expect(chips(page).filter(([, pressed]) => pressed === "true")).toEqual([["RB", "true"]]);
  const shown = rows(page);
  expect(shown).toHaveLength(5);
  expect(shown.every((row) => row.querySelector(".pos")?.textContent === "RB")).toBe(true);
  expect(shown.map((row) => row.querySelector(".rank")?.textContent)).toEqual([
    "2",
    "5",
    "7",
    "9",
    "11",
  ]);
  expect(page.stored(FILTER_KEY)).toBe("RB");
  // The chip strip itself is not rebuilt for a tap: the button stays.
  expect(page.byId("available-filter").contains(rb ?? null)).toBe(true);

  document.body.innerHTML = "";
  const reloaded = await picksTab(board(), { [FILTER_KEY]: "RB" });
  expect(chips(reloaded.page).filter(([, pressed]) => pressed === "true")).toEqual([
    ["RB", "true"],
  ]);
  expect(rows(reloaded.page).every((row) => row.querySelector(".pos")?.textContent === "RB")).toBe(
    true,
  );
});

it("falls back to All when the remembered position has left the board", async () => {
  const { page } = await picksTab(board(), { [FILTER_KEY]: "K" });
  expect(chips(page)[0]).toEqual(["All", "true"]);
  expect(rows(page)).toHaveLength(12);
});

it("keeps the same row nodes when the board has not changed, and swaps them when it has", async () => {
  const { page, feed } = await picksTab(board());
  const before = rows(page);
  feed(board());
  document.querySelector<HTMLButtonElement>('[data-tab="picks"]')?.click();
  const after = rows(page);
  expect(after).toHaveLength(before.length);
  after.forEach((row, i) => expect(row).toBe(before[i]));
  // The first player is drafted: the list moves on, so the nodes do too.
  feed(board().slice(1));
  const moved = rows(page);
  expect(moved[0]).not.toBe(before[0]);
  expect(moved[0].querySelector(".rank")?.textContent).toBe("2");
});

it("says so when there is nobody on the board, or no draft at all", async () => {
  const { page, feed } = await picksTab([]);
  expect(page.byId("available").textContent).toBe("No players on the board yet.");
  expect(page.byId("available").querySelector("li")?.className).toBe("muted");
  expect(chips(page)).toEqual([["All", "true"]]);
  feed(board());
  expect(rows(page)).toHaveLength(12);
  FakeSocket.instances[0]?.frame("draft-updated", null);
  expect(page.byId("available").textContent).toBe("No players on the board yet.");
});

/** The Show all button after the list, or null when the list already fits. */
const showAll = (page: Booted): HTMLButtonElement | null => {
  const next = page.byId("available").nextElementSibling;
  return next?.classList.contains("show-all") ? (next as HTMLButtonElement) : null;
};

it("folds the list at twelve and opens it all the way out on Show all", async () => {
  const { page, feed } = await picksTab(board());
  expect(rows(page)).toHaveLength(12);
  const button = showAll(page);
  expect(button?.className).toBe("show-all");
  expect(button?.type).toBe("button");
  expect(button?.textContent).toBe("Show all 13");
  expect(button?.getAttribute("aria-expanded")).toBe("false");
  button?.click();
  const opened = rows(page);
  expect(opened).toHaveLength(13);
  expect(opened[12].querySelector(".name")?.textContent).toBe("Saints");
  expect(showAll(page)?.textContent).toBe("Show fewer");
  expect(showAll(page)?.getAttribute("aria-expanded")).toBe("true");
  // The same board again, on a clock tick say: still open, same nodes.
  feed(board());
  document.querySelector<HTMLButtonElement>('[data-tab="picks"]')?.click();
  const again = rows(page);
  expect(again).toHaveLength(13);
  again.forEach((row, i) => expect(row).toBe(opened[i]));
  showAll(page)?.click();
  expect(rows(page)).toHaveLength(12);
  expect(showAll(page)?.textContent).toBe("Show all 13");
});

it("folds the list back when a chip is tapped, and leaves the button out when it all fits", async () => {
  const { page } = await picksTab(board());
  showAll(page)?.click();
  expect(rows(page)).toHaveLength(13);
  const chip = (label: string) =>
    [...page.byId("available-filter").querySelectorAll("button")].find(
      (b) => b.textContent === label,
    );
  chip("RB")?.click();
  expect(rows(page)).toHaveLength(5);
  expect(showAll(page)).toBeNull();
  chip("All")?.click();
  expect(rows(page)).toHaveLength(12);
  expect(showAll(page)?.textContent).toBe("Show all 13");
});

it("refreshes displayed player details even when ranking facts are unchanged", async () => {
  const original = player(1, "WR");
  const { page, feed } = await picksTab([original]);
  feed([{ ...original, name: "Updated player", position: "RB", team: "BUF", bye_week: 12 }]);
  expect(rows(page)[0]).toHaveTextContent("Updated player");
  expect(rows(page)[0].querySelector(".pos")).toHaveTextContent("RB");
  expect(rows(page)[0]).toHaveTextContent("BUF");
  expect(rows(page)[0]).toHaveTextContent("Bye 12");
});
