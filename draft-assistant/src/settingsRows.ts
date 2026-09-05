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
  onLeaveHost: () => void;
  onDismiss: () => void;
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
          ? "Headshots from Sleeper, saved on this Mac after the first look"
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
      label: "Version",
      note: "Draft Assistant",
      value: `v${input.appVersion}`,
      on: false,
      onSelect: input.onDismiss,
    },
  );

  return rows;
}
