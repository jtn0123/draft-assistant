// The settings menu's rows, built from what the shell knows.
//
// Lifted out of App.tsx when the companion feature added three more of them:
// the list is a description of the app's whole surface and was the single
// biggest thing in that file. It is a pure function of the state it is handed,
// so what a row says can be read (and tested) without rendering anything.

import type { SettingsRow } from "./components/Header";
import type { AvatarMode } from "./avatars";
import type { DraftView, YahooStatus } from "./types";
import type { ThemePreference } from "./theme";
import { age } from "./format";
import { importNote } from "./secondOpinionImport";
import { platformName } from "./leagues";
import { yahooNote } from "./yahoo";

export interface SettingsRowInput {
  view: DraftView;
  chime: boolean;
  polling: boolean;
  lastSyncAt: number | null;
  leagueCount: number;
  yahoo: YahooStatus | null;
  yahooConnected: boolean;
  busy: boolean;
  avatars: AvatarMode;
  preference: ThemePreference;
  theme: "light" | "dark";
  appVersion: string;
  /** The host this app follows, when it is a follower. Everything the host
   *  owns — the league, the keys, the budget, Yahoo — is left off the menu
   *  rather than shown disabled: a follower cannot act on any of it. */
  hostName: string | null;
  /** Whether the companion server is currently serving. */
  companionOn: boolean;
  onChime: (next: boolean) => void;
  onTogglePolling: () => void;
  onLeaguePicker: () => void;
  onYahoo: () => void;
  onRefreshData: () => void;
  onExport: () => void;
  onImportCsv: () => void;
  onClearKeepers: () => void;
  onAvatars: (next: AvatarMode) => void;
  onAppearance: () => void;
  onCompanion: () => void;
  onJoinHost: () => void;
  /** Open Settings -> "Diagnostics…". */
  onDiagnostics: () => void;
  onLeaveHost: () => void;
  onDismiss: () => void;
  /** Ask for the Sleeper username again. Optional because the shell wires it
   *  up separately; without it the row stays off the menu rather than
   *  offering a button that does nothing. */
  onSetUsername?: () => void;
}

/** Where headshots come from, named by platform.
 *
 * Sleeper serves them; Yahoo's do not come from Sleeper at all, and telling a
 * Yahoo-only user that their pictures come from a service they have never
 * connected read as a bug in the app. */
function headshotNote(platform: string): string {
  const source = platform === "yahoo" ? "your league" : "Sleeper";
  return `Headshots from ${source}, saved on this Mac after the first look`;
}

export function buildSettingsRows(input: SettingsRowInput): SettingsRow[] {
  const follower = input.hostName !== null;
  const rows: SettingsRow[] = [
    {
      label: "Pick chime",
      note: "Sound when you're on the clock",
      value: input.chime ? "On" : "Off",
      on: input.chime,
      onSelect: () => input.onChime(!input.chime),
    },
  ];

  if (!follower) {
    rows.push(
      {
        label: "Live sync",
        note: input.polling
          ? `Last sync ${age(input.lastSyncAt)}`
          : `Not polling ${platformName(input.view.league.platform)}`,
        value: input.polling ? "On" : "Off",
        on: input.polling,
        onSelect: input.onTogglePolling,
      },
      {
        label: "League",
        note:
          input.leagueCount > 1 ? `${input.leagueCount} leagues loaded` : "Switch or add a league",
        value: "Switch",
        on: false,
        onSelect: input.onLeaguePicker,
      },
      {
        label: "Yahoo",
        note: yahooNote(input.yahoo),
        value: input.yahooConnected ? "Connected" : "Connect",
        on: input.yahooConnected,
        onSelect: input.onYahoo,
      },
    );
    // The username is asked for once, on the first-launch screen, and never
    // again. Skip it there — or mistype it — and the app has no way to know
    // which team is yours, the roster panel stays empty, and there was no
    // route back to the question short of clearing the config by hand. Yahoo
    // learns the same thing from the connected account, so the row is
    // Sleeper's alone.
    if (input.view.league.platform === "sleeper" && input.onSetUsername !== undefined) {
      rows.push({
        label: "Sleeper username…",
        note:
          input.view.my_roster === null
            ? "Not set, so no team on the board is marked as yours"
            : "Change which team on the board is yours",
        value: input.view.my_roster === null ? "Set" : "Change",
        on: input.view.my_roster !== null,
        onSelect: input.onSetUsername,
      });
    }
  }

  rows.push({
    label: "Phone & second screen",
    note: follower
      ? "Hosted elsewhere — the host serves the phones"
      : "Let a phone or another Mac watch this league",
    value: follower ? "Host's" : input.companionOn ? "On" : "Off",
    on: input.companionOn && !follower,
    onSelect: follower ? input.onDismiss : input.onCompanion,
  });

  if (follower) {
    rows.push({
      label: "Leave host",
      note: `Following ${input.hostName ?? ""} — go back to this Mac's own leagues`,
      value: "Leave",
      on: true,
      onSelect: input.onLeaveHost,
    });
  } else {
    rows.push({
      label: "Join another Draft Assistant…",
      note: "Watch someone else's league on this Mac",
      value: "Join",
      on: false,
      onSelect: input.onJoinHost,
    });
  }

  if (!follower) {
    rows.push(
      {
        label: "Refresh data",
        note: "Re-fetch projections and rebuild the board",
        value: input.busy ? "…" : "Sync",
        on: false,
        onSelect: input.onRefreshData,
      },
      {
        label: "Export state",
        note: "Full JSON dump of everything on screen",
        value: "JSON",
        on: false,
        onSelect: input.onExport,
      },
      {
        label: "Clear detected keepers",
        note:
          input.view.draft.keeper_picks.length === 0
            ? "Nothing is marked as kept in this draft"
            : `${input.view.draft.keeper_picks.length} picks marked as kept; judge them again`,
        value: "Clear",
        on: false,
        onSelect: input.onClearKeepers,
      },
      {
        label: "Import projections CSV…",
        note: importNote(
          input.view.data_health.second_opinion_loaded_at,
          input.view.available.find((p) => p.second_opinion !== null)?.second_opinion?.source ??
            null,
        ),
        value: "Choose",
        on: input.view.data_health.second_opinion_loaded_at !== null,
        onSelect: input.onImportCsv,
      },
    );
  }

  rows.push(
    {
      label: "Player pictures",
      note:
        input.avatars === "headshots"
          ? headshotNote(input.view.league.platform)
          : "Team logos only — no photo downloads",
      value: input.avatars === "headshots" ? "Headshots" : "Team logos",
      on: input.avatars === "headshots",
      onSelect: () => input.onAvatars(input.avatars === "headshots" ? "logos" : "headshots"),
    },
    {
      label: "Appearance",
      note:
        input.preference === "system"
          ? "Following your system setting"
          : "Overriding your system setting",
      value:
        input.preference === "system"
          ? `System (${input.theme})`
          : input.theme === "dark"
            ? "Dark"
            : "Light",
      on: input.theme === "dark",
      onSelect: input.onAppearance,
    },
    {
      label: "Diagnostics…",
      note: "What this app knows about itself, and the log",
      value: "Show",
      on: false,
      onSelect: input.onDiagnostics,
    },
    {
      label: "Version",
      note: "Draft Assistant",
      value: `v${input.appVersion}`,
      on: false,
      onSelect: input.onDismiss,
    },
  );

  return rows;
}
