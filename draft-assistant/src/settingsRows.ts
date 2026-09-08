// The settings menu's rows, built from what the shell knows.
//
// Lifted out of App.tsx when the companion feature added three more of them:
// the list is a description of the app's whole surface and was the single
// biggest thing in that file. It is a pure function of the state it is handed,
// so what a row says can be read (and tested) without rendering anything.

import type { SettingsOption, SettingsRow } from "./components/Header";
import type { AvatarMode } from "./avatars";
import type { DraftView, YahooStatus } from "./types";
import type { ThemePreference } from "./theme";
import { age } from "./format";
import { importNote } from "./secondOpinionImport";
import { platformName } from "./leagues";
import { updateRow } from "./updateRow";
import type { UpdateRowState } from "./useUpdateRow";
import { yahooNote } from "./yahoo";

export interface SettingsRowInput {
  view: DraftView;
  chime: boolean;
  /** Whether the header carries its Ask AI button. */
  ask: boolean;
  polling: boolean;
  lastSyncAt: number | null;
  leagueCount: number;
  yahoo: YahooStatus | null;
  yahooConnected: boolean;
  busy: boolean;
  avatars: AvatarMode;
  preference: ThemePreference;
  theme: "light" | "dark";
  /** The running version and the "Check for updates" row's state. The row is
   *  left off where `supported` is false: the browser preview and a follower
   *  have no updater of their own to ask. */
  updates: UpdateRowState;
  /** The host this app follows, when it is a follower. Everything the host
   *  owns — the league, provider credentials, Yahoo — is left off the menu
   *  rather than shown disabled: a follower cannot act on any of it. */
  hostName: string | null;
  /** Whether the companion server is currently serving. */
  companionOn: boolean;
  onChime: (next: boolean) => void;
  onAsk: (next: boolean) => void;
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
 * Sleeper's photo library is the only source there is. A Yahoo player gets a
 * photo only when the app matches them to a Sleeper player, and none at all
 * otherwise; the row used to say "from your league", which is not where any
 * of them come from. */
export function headshotNote(platform: string): string {
  if (platform === "yahoo") {
    return "Sleeper's photos for players the app can match, none for the rest; saved on this Mac";
  }
  return "Headshots from Sleeper, saved on this Mac after the first look";
}

/** The order the shell's `onAppearance` steps through, from theme.ts. */
const APPEARANCES: { preference: ThemePreference; label: string }[] = [
  { preference: "system", label: "System" },
  { preference: "light", label: "Light" },
  { preference: "dark", label: "Dark" },
];

/** The Appearance picker's three choices.
 *
 * The shell hands the menu one action, the step system -> light -> dark ->
 * system that the row used to cycle on every click. A choice is reached by
 * taking as many of those steps as it is away, which is none when it is the
 * one already showing, so each choice can be a radio button in its own right
 * without asking the shell for a second wire. */
export function appearanceOptions(input: SettingsRowInput): SettingsOption[] {
  const at = APPEARANCES.findIndex((a) => a.preference === input.preference);
  return APPEARANCES.map((choice, index) => ({
    id: choice.preference,
    label: choice.label,
    on: index === at,
    onSelect: () => {
      const steps = (index - at + APPEARANCES.length) % APPEARANCES.length;
      for (let taken = 0; taken < steps; taken += 1) input.onAppearance();
    },
  }));
}

/** The four rows the header's own menu shows, plus the way into the rest.
 *
 * The gear menu is a shortcut, not a second settings surface: the things a
 * user reaches for mid-draft, and one row that opens everything else. Kept
 * beside the full list so the two cannot drift apart, and out of the shell,
 * which is at its line cap. */
export function headerMenuRows(rows: SettingsRow[], onAllSettings: () => void): SettingsRow[] {
  const quick = ["chime", "ask", "polling", "refresh"];
  return [
    ...rows.filter((row) => quick.includes(row.id)),
    {
      id: "all-settings",
      kind: "action",
      label: "All settings…",
      note: "Identity, remote connections, appearance and diagnostics",
      value: "Open",
      on: false,
      onSelect: onAllSettings,
    },
  ];
}

export function buildSettingsRows(input: SettingsRowInput): SettingsRow[] {
  const follower = input.hostName !== null;
  const rows: SettingsRow[] = [
    {
      id: "chime",
      kind: "toggle",
      label: "Pick chime",
      note: "Sound when you're on the clock",
      value: input.chime ? "On" : "Off",
      on: input.chime,
      onSelect: () => input.onChime(!input.chime),
    },
    {
      id: "ask",
      kind: "toggle",
      label: "Ask AI button",
      note: input.ask ? "Shown in the header" : "Hidden from the header",
      value: input.ask ? "On" : "Off",
      on: input.ask,
      onSelect: () => input.onAsk(!input.ask),
    },
  ];

  if (!follower) {
    rows.push(
      {
        id: "polling",
        kind: "toggle",
        label: "Live sync",
        note: input.polling
          ? `Last sync ${age(input.lastSyncAt)}`
          : `Not polling ${platformName(input.view.league.platform)}`,
        value: input.polling ? "On" : "Off",
        on: input.polling,
        onSelect: input.onTogglePolling,
      },
      {
        id: "league",
        kind: "action",
        label: "League",
        note:
          input.leagueCount > 1 ? `${input.leagueCount} leagues loaded` : "Switch or add a league",
        value: "Switch",
        on: false,
        onSelect: input.onLeaguePicker,
      },
      {
        id: "yahoo",
        kind: "action",
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
        id: "username",
        kind: "action",
        label: "Sleeper username…",
        note:
          input.view.my_roster === null
            ? "Choose your account below; draft seats may still be pending"
            : "Change which team on the board is yours",
        value: input.view.my_roster === null ? "Set" : "Change",
        on: input.view.my_roster !== null,
        onSelect: input.onSetUsername,
      });
    }
  }

  // Opens the companion dialog rather than flipping the server, so it is an
  // action; the value still says whether the phones are being served.
  rows.push({
    id: "companion",
    kind: "action",
    label: "Phone & second screen",
    note: follower
      ? "Hosted elsewhere. The host serves the phones"
      : "Let a phone or another Mac watch this league",
    value: follower ? "Host's" : input.companionOn ? "On" : "Off",
    on: input.companionOn && !follower,
    onSelect: follower ? input.onDismiss : input.onCompanion,
  });

  if (follower) {
    rows.push({
      id: "leave-host",
      kind: "action",
      label: "Leave host",
      note: `Following ${input.hostName ?? ""}. Go back to this Mac's own leagues`,
      value: "Leave",
      on: true,
      onSelect: input.onLeaveHost,
    });
  } else {
    rows.push({
      id: "join-host",
      kind: "action",
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
        id: "refresh",
        kind: "action",
        label: "Refresh data",
        note: "Re-fetch projections and rebuild the board",
        value: input.busy ? "…" : "Sync",
        on: false,
        onSelect: input.onRefreshData,
      },
      {
        id: "export",
        kind: "action",
        label: "Export state",
        note: "Full JSON dump of everything on screen",
        value: "JSON",
        on: false,
        onSelect: input.onExport,
      },
      {
        id: "clear-keepers",
        kind: "action",
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
        id: "import-csv",
        kind: "action",
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
      id: "avatars",
      kind: "toggle",
      label: "Player pictures",
      note:
        input.avatars === "headshots"
          ? headshotNote(input.view.league.platform)
          : "Team logos only, no photo downloads",
      value: input.avatars === "headshots" ? "Headshots" : "Team logos",
      on: input.avatars === "headshots",
      onSelect: () => input.onAvatars(input.avatars === "headshots" ? "logos" : "headshots"),
    },
    {
      id: "appearance",
      kind: "radio",
      label: "Appearance",
      note:
        input.preference === "system"
          ? `Following your system setting, ${input.theme} right now`
          : "Overriding your system setting",
      value: input.preference === "system" ? `System (${input.theme})` : input.theme,
      on: input.theme === "dark",
      onSelect: input.onAppearance,
      options: appearanceOptions(input),
    },
    {
      id: "diagnostics",
      kind: "action",
      label: "Diagnostics…",
      note: "What this app knows about itself, and the log",
      value: "Show",
      on: false,
      onSelect: input.onDiagnostics,
    },
  );

  // Desktop only. A follower's `api` speaks to the host, and the browser
  // preview has no shell, so neither has a check to offer; the row is left
  // off rather than shown disabled, as every other host-owned row is.
  if (input.updates.supported) {
    rows.push(updateRow(input.updates.current, input.updates.state, input.updates.select));
  }

  rows.push({
    id: "version",
    kind: "action",
    label: "Version",
    note: "Draft Assistant",
    value: `v${input.updates.current}`,
    on: false,
    onSelect: input.onDismiss,
  });

  return rows;
}
