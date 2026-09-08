import { describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView } from "./types";
import { appearanceOptions, buildSettingsRows, type SettingsRowInput } from "./settingsRows";
import { IDLE } from "./updateRow";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

/** Everything the menu is handed, with nothing switched on and every action a
 *  spy — so a test only has to say the one thing it is about. */
function input(overrides: Partial<SettingsRowInput> = {}): SettingsRowInput {
  return {
    view: view(),
    chime: false,
    ask: false,
    polling: false,
    lastSyncAt: null,
    leagueCount: 1,
    yahoo: null,
    yahooConnected: false,
    busy: false,
    avatars: "logos",
    preference: "system",
    theme: "light",
    updates: { current: "0.2.0", state: IDLE, supported: true, select: vi.fn() },
    hostName: null,
    companionOn: false,
    onChime: vi.fn(),
    onAsk: vi.fn(),
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

  it("tells a Yahoo player which of their players get a photo, and from where", () => {
    // The row said "from your league". Nothing comes from Yahoo: a Yahoo
    // player the app matches to a Sleeper one gets Sleeper's photo, and the
    // rest get none, so that is what the row has to say.
    const state = input({ avatars: "headshots" });
    state.view.league.platform = "yahoo";
    const note = row(buildSettingsRows(state), "Player pictures")?.note;
    expect(note).toContain("Sleeper's photos for players the app can match");
    expect(note).toContain("none for the rest");
    expect(note).not.toContain("from your league");
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

  it("offers the identity picker on a Sleeper league while draft seats are pending", () => {
    const onSetUsername = vi.fn();
    const state = input({ onSetUsername });
    state.view.league.platform = "sleeper";
    state.view.my_roster = null;

    const set = row(buildSettingsRows(state), LABEL);
    expect(set?.note).toContain("Choose your account below");
    expect(set?.note).toContain("draft seats may still be pending");
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

// The updater plugin was registered and granted for a whole release cycle
// and nothing on the menu called it, so a signed release could reach nobody.
describe("checking for updates from the menu", () => {
  it("offers the row on the desktop and hands it the hook's action", () => {
    const select = vi.fn();
    const state = input({ updates: { current: "0.2.0", state: IDLE, supported: true, select } });

    const check = row(buildSettingsRows(state), "Check for updates");
    expect(check?.value).toBe("Check");
    check?.onSelect();
    expect(select).toHaveBeenCalledTimes(1);
  });

  it("shows what the check found in place of the offer", () => {
    const state = input({
      updates: {
        current: "0.2.0",
        state: { kind: "available", version: "0.3.1", notes: "- Keeper guard" },
        supported: true,
        select: vi.fn(),
      },
    });
    const rows = buildSettingsRows(state);
    expect(row(rows, "Update to 0.3.1")?.note).toBe("Keeper guard");
    expect(row(rows, "Check for updates")).toBeUndefined();
  });

  it("leaves the row off where there is no updater to ask, and keeps the version line", () => {
    const state = input({
      updates: { current: "0.2.0", state: IDLE, supported: false, select: vi.fn() },
    });
    const rows = buildSettingsRows(state);
    expect(row(rows, "Check for updates")).toBeUndefined();
    expect(row(rows, "Version")?.value).toBe("v0.2.0");
  });

  it("sits just above the version line, the last two things on the menu", () => {
    const labels = buildSettingsRows(input()).map((r) => r.label);
    expect(labels.slice(-2)).toEqual(["Check for updates", "Version"]);
  });
});

// A screen reader names a toggle, an action and a picker differently, and the
// menu can only tell them apart if every row says which it is.
describe("what kind of thing each row is", () => {
  it("marks the settings with a state as toggles and the rest as actions", () => {
    const rows = buildSettingsRows(input({ onSetUsername: vi.fn() }));
    const kind = (label: string) => row(rows, label)?.kind;
    expect(kind("Pick chime")).toBe("toggle");
    expect(kind("Ask AI button")).toBe("toggle");
    expect(kind("Live sync")).toBe("toggle");
    expect(kind("Player pictures")).toBe("toggle");
    for (const label of [
      "League",
      "Yahoo",
      "Sleeper username…",
      "Phone & second screen",
      "Join another Draft Assistant…",
      "Refresh data",
      "Export state",
      "Clear detected keepers",
      "Import projections CSV…",
      "Diagnostics…",
      "Check for updates",
      "Version",
    ]) {
      expect(kind(label), label).toBe("action");
    }
    expect(kind("Appearance")).toBe("radio");
  });

  it("gives every row an id of its own, so React never keys on a label", () => {
    const ids = buildSettingsRows(input({ onSetUsername: vi.fn() })).map((r) => r.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids.every((id) => id !== "")).toBe(true);
  });

  it("keeps the updater row's id while its label moves", () => {
    const at = (state: SettingsRowInput["updates"]["state"]) =>
      buildSettingsRows(
        input({ updates: { current: "0.2.0", state, supported: true, select: vi.fn() } }),
      )
        .filter((r) => /Check for updates|Up to date|Update to/.test(r.label))
        .map((r) => r.id);
    expect(at(IDLE)).toEqual(["updates"]);
    expect(at({ kind: "checking" })).toEqual(["updates"]);
    expect(at({ kind: "available", version: "0.3.2", notes: null })).toEqual(["updates"]);
  });
});

describe("the appearance picker", () => {
  it("offers system, light and dark, with the current one checked", () => {
    const options = appearanceOptions(input({ preference: "light", theme: "light" }));
    expect(options.map((o) => o.label)).toEqual(["System", "Light", "Dark"]);
    expect(options.map((o) => o.on)).toEqual([false, true, false]);
    const onRow = row(buildSettingsRows(input({ preference: "light" })), "Appearance")?.options;
    expect(onRow?.map((o) => [o.id, o.on])).toEqual(options.map((o) => [o.id, o.on]));
  });

  it("reaches a choice by stepping the shell's one action as far as it is away", () => {
    // The shell wires system -> light -> dark -> system; dark from system is
    // two steps, system from dark is one, and the one already showing is none.
    const fromSystem = input({ preference: "system" });
    appearanceOptions(fromSystem)[2].onSelect();
    expect(fromSystem.onAppearance).toHaveBeenCalledTimes(2);

    const fromDark = input({ preference: "dark", theme: "dark" });
    appearanceOptions(fromDark)[0].onSelect();
    expect(fromDark.onAppearance).toHaveBeenCalledTimes(1);

    const already = input({ preference: "dark", theme: "dark" });
    appearanceOptions(already)[2].onSelect();
    expect(already.onAppearance).not.toHaveBeenCalled();
  });

  it("says what the system setting resolves to while following it", () => {
    const following = row(
      buildSettingsRows(input({ preference: "system", theme: "dark" })),
      "Appearance",
    );
    expect(following?.note).toBe("Following your system setting, dark right now");
    const overriding = row(
      buildSettingsRows(input({ preference: "dark", theme: "dark" })),
      "Appearance",
    );
    expect(overriding?.note).toBe("Overriding your system setting");
  });
});
