import { afterEach, expect, it } from "vitest";
import { boot, FakeSocket, flush, okJson } from "./test/companionPageHarness";

// A reload should land where the phone was: same tab, same distance down.

const view = {
  draft: {
    status: "drafting",
    teams: 12,
    rounds: 15,
    pick_timer: 60,
    current_pick: 5,
    current_round: 1,
    on_clock_slot: 5,
    on_clock_name: "Rob",
    my_slot: 3,
    is_my_pick: false,
    picks_until_mine: 7,
    my_next_picks: [12],
    total_picks_made: 4,
    paused: false,
    clock_deadline_ms: null,
    pick_slot_overrides: {},
    keeper_picks: [],
    is_auction: false,
  },
  recommendations: [],
  recent_picks: [],
  rosters: [],
  available: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: 0 },
};

afterEach(() => {
  document.body.innerHTML = "";
});

it("remembers the tab on the device and opens on it after a reload", async () => {
  const page = boot(() => okJson(null));
  await flush();
  document.querySelector<HTMLButtonElement>('[data-tab="picks"]')?.click();
  expect(page.stored("da.companion.tab")).toBe("picks");
  document.body.innerHTML = "";
  boot(() => okJson(null), { saved: { "da.companion.token": "tok", "da.companion.tab": "picks" } });
  await flush();
  expect(document.querySelector('[data-tab="picks"]')).toHaveAttribute("aria-current", "page");
  expect(document.getElementById("tab-picks")).not.toHaveAttribute("hidden");
});

it("never opens on the Week tab, which is hidden until the season arrives", async () => {
  boot(() => okJson(null), { saved: { "da.companion.token": "tok", "da.companion.tab": "week" } });
  await flush();
  expect(document.querySelector('[data-tab="now"]')).toHaveAttribute("aria-current", "page");
});

it("saves how far down each tab was, a little after the thumb stops", () => {
  const page = boot(() => okJson(null));
  page.scroll(340);
  expect(page.session("da.companion.scroll.now")).toBeNull();
  page.fireTimers();
  expect(page.session("da.companion.scroll.now")).toBe("340");
});

it("scrolls back to the saved spot once the tab has real data, and only once", async () => {
  const page = boot(() => okJson(view), {
    saved: { "da.companion.token": "tok" },
    session: { "da.companion.scroll.now": "220" },
  });
  expect(page.scrolledTo).not.toHaveBeenCalled();
  await flush();
  expect(page.scrolledTo).toHaveBeenCalledWith(0, 220);
  FakeSocket.instances[0]?.open();
  FakeSocket.instances[0]?.frame("draft-updated", view);
  expect(page.scrolledTo).toHaveBeenCalledTimes(1);
});
