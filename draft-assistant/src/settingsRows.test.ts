import { describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView } from "./types";
import { buildSettingsRows, type SettingsRowInput } from "./settingsRows";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

/** Everything the menu is handed, with nothing switched on and every action a
 *  spy — so a test only has to say the one thing it is about. */
function input(overrides: Partial<SettingsRowInput> = {}): SettingsRowInput {
  return {
    view: view(),
    chime: false,
    polling: false,
    lastSyncAt: null,
    leagueCount: 1,
    yahoo: null,
    yahooConnected: false,
    busy: false,
    avatars: "logos",
    preference: "system",
    theme: "light",
    appVersion: "0.2.0",
    hostName: null,
    companionOn: false,
    onChime: vi.fn(),
    onTogglePolling: vi.fn(),
    onLeaguePicker: vi.fn(),
    onYahoo: vi.fn(),
    onRefreshData: vi.fn(),
    onExport: vi.fn(),
    onImportCsv: vi.fn(),
    onClearKeepers: vi.fn(),
    onAvatars: vi.fn(),
    onAppearance: vi.fn(),
    onCompanion: vi.fn(),
    onJoinHost: vi.fn(),
    onDiagnostics: vi.fn(),
    onLeaveHost: vi.fn(),
    onDismiss: vi.fn(),
    ...overrides,
  };
}

const row = (rows: ReturnType<typeof buildSettingsRows>, label: string) =>
  rows.find((r) => r.label === label);

// A keeper judgement is deliberately never revisited, so a league branded
// from one bad pick list stayed branded through every relaunch. The menu is
// the only way out of that, and only the host has one to offer.
describe("clearing detected keepers", () => {
  it("offers the host a way to undo a wrong keeper judgement", () => {
    const state = input();
    state.view.draft.keeper_picks = [11, 20, 177];
    const clear = row(buildSettingsRows(state), "Clear detected keepers");

    expect(clear).toBeDefined();
    expect(clear?.note).toContain("3 picks marked as kept");
    clear?.onSelect();
    expect(state.onClearKeepers).toHaveBeenCalledTimes(1);
  });

  it("says so plainly when this draft has no keepers on it", () => {
    const state = input();
    state.view.draft.keeper_picks = [];

    expect(row(buildSettingsRows(state), "Clear detected keepers")?.note).toBe(
      "Nothing is marked as kept in this draft",
    );
  });

  it("leaves the row off a follower, which owns none of this", () => {
    const rows = buildSettingsRows(input({ hostName: "Justin's Mac" }));

    expect(row(rows, "Clear detected keepers")).toBeUndefined();
    // The follower's menu is not simply empty: it still has its own rows.
    expect(row(rows, "Leave host")).toBeDefined();
  });
});

// "Headshots from Sleeper" was written on the menu of a Yahoo-only user who
// has never connected Sleeper and never will — which reads as the app having
// loaded the wrong league.
describe("where the player pictures come from", () => {
  it("names Sleeper on a Sleeper league", () => {
    const state = input({ avatars: "headshots" });
    state.view.league.platform = "sleeper";
    expect(row(buildSettingsRows(state), "Player pictures")?.note).toContain("from Sleeper");
  });

  it("does not name Sleeper on a Yahoo league", () => {
    const state = input({ avatars: "headshots" });
    state.view.league.platform = "yahoo";
    const note = row(buildSettingsRows(state), "Player pictures")?.note;
    expect(note).not.toContain("Sleeper");
    expect(note).toContain("saved on this Mac");
  });

  it("says nothing about a source while only logos are drawn", () => {
    const state = input({ avatars: "logos" });
    state.view.league.platform = "yahoo";
    expect(row(buildSettingsRows(state), "Player pictures")?.note).not.toContain("Sleeper");
  });
});

// The username is asked for once, on the first-launch screen, and never again.
// Skip it there and the roster panel stays empty for the whole season, with no
// route back to the question short of editing the config by hand.
describe("setting the Sleeper username after the first launch", () => {
  const LABEL = "Sleeper username…";

  it("offers the row on a Sleeper league, and says no team is claimed yet", () => {
    const onSetUsername = vi.fn();
    const state = input({ onSetUsername });
    state.view.league.platform = "sleeper";
    state.view.my_roster = null;

    const set = row(buildSettingsRows(state), LABEL);
    expect(set?.note).toContain("no team on the board is marked as yours");
    expect(set?.value).toBe("Set");
    set?.onSelect();
    expect(onSetUsername).toHaveBeenCalled();
  });

  it("offers to change it once a team is claimed", () => {
    const state = input({ onSetUsername: vi.fn() });
    state.view.league.platform = "sleeper";

    const set = row(buildSettingsRows(state), LABEL);
    expect(set?.value).toBe("Change");
    expect(set?.on).toBe(true);
  });

  it("leaves it off a Yahoo league, which learns the team from the account", () => {
    const state = input({ onSetUsername: vi.fn() });
    state.view.league.platform = "yahoo";
    expect(row(buildSettingsRows(state), LABEL)).toBeUndefined();
  });

  it("leaves it off a follower, who owns none of the host's league", () => {
    const state = input({ onSetUsername: vi.fn(), hostName: "Justin's Mac" });
    state.view.league.platform = "sleeper";
    expect(row(buildSettingsRows(state), LABEL)).toBeUndefined();
  });

  it("shows no row at all until the shell has an action to give it", () => {
    const state = input();
    state.view.league.platform = "sleeper";
    expect(row(buildSettingsRows(state), LABEL)).toBeUndefined();
  });
});
