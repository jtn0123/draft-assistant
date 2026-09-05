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
