import { runCompanionScript } from "./test/companionScript";
import { beforeEach, expect, it } from "vitest";

interface QueuePick {
  pick: number;
  slot: number;
  name: string;
  mine: boolean;
}
interface Extras {
  mobilePickQueue: (view: unknown) => QueuePick[];
  renderMobileDraft: (view: unknown, now: number) => void;
}
const load = () => {
  const window = { Companion: {} as Extras };
  for (const file of ["helpers.js", "clock.js"]) {
    runCompanionScript(file, {
      window,
      document,
    });
  }
  return window.Companion;
};
const draft = () => ({
  status: "drafting",
  teams: 4,
  rounds: 3,
  current_pick: 4,
  my_slot: 2,
  keeper_picks: [6],
  pick_slot_overrides: { "7": 1 },
  my_next_picks: [10],
  picks_until_mine: 5,
  pick_timer: 60,
  clock_deadline_ms: 60000,
  total_picks_made: 3,
});
const view = () => ({
  draft: draft(),
  rosters: [1, 2, 3, 4].map((slot) => ({ slot, display_name: `Team ${slot}` })),
});
beforeEach(() => {
  document.body.innerHTML = '<div id="tab-now"><div id="clock-strip"></div></div>';
});
it("matches snake turnarounds and traded picks while skipping keepers", () => {
  const queue = load().mobilePickQueue(view());
  expect(queue.slice(0, 4).map((p) => [p.pick, p.slot])).toEqual([
    [4, 4],
    [5, 4],
    [7, 1],
    [8, 1],
  ]);
  expect(queue[2].name).toBe("Team 1");
  expect(queue.find((p) => p.pick === 10)?.mine).toBe(true);
});
it("does not invent future turns for completed, auction or invalid drafts", () => {
  const { mobilePickQueue } = load();
  for (const change of [{ status: "complete" }, { is_auction: true }, { teams: 0 }]) {
    expect(mobilePickQueue({ ...view(), draft: { ...draft(), ...change } })).toEqual([]);
  }
});
it("ticks the countdown, clamps expiry, and preserves expanded queue DOM", () => {
  const { renderMobileDraft } = load();
  renderMobileDraft(view(), 15000);
  expect(document.querySelector(".mobile-timer-value")?.textContent).toBe("0:45");
  const details = document.querySelector("details")!;
  details.open = true;
  renderMobileDraft(view(), 60001);
  expect(document.querySelector(".mobile-timer-value")?.textContent).toBe("0:00");
  expect(document.querySelector("details")).toBe(details);
  expect(details.open).toBe(true);
  expect(document.querySelector("#mobile-up-next")?.textContent).toContain("3.02");
});
it("suppresses ticking when paused or waiting and explains an unposted order", () => {
  const { renderMobileDraft } = load();
  renderMobileDraft({ ...view(), draft: { ...draft(), paused: true } }, 1000);
  expect(document.querySelector(".mobile-timer-value")).toBeNull();
  renderMobileDraft(
    { draft: { ...draft(), status: "pre_draft", total_picks_made: 0 }, rosters: [] },
    1000,
  );
  expect(document.querySelector("#mobile-up-next")?.textContent).toContain(
    "Waiting for the draft order",
  );
  expect(document.querySelector(".mobile-timer-value")).toBeNull();
});
