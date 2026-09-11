// Draft payload types and schema versions come directly from Rust.
export * from "./draft-contract.generated";
import type { DraftView, Platform } from "./draft-contract.generated";

export interface PollHealth {
  last_success_at: number | null;
  consecutive_failures: number;
  last_error: string | null;
}

/** What "Import projections CSV…" reports back. */
export interface SecondOpinionImport {
  matched: number;
  total: number;
  /** The sentence for the toast, written by the backend. */
  message: string;
  /** Rows the backend would not rank: points invented from an ADP curve, or
   *  defences ranked off a week-one matchup page. Zero for an older file. */
  excluded_rows: number;
  /** Which of them, in words — "57 estimated from ADP". Null when none. */
  excluded_reason: string | null;
  view: DraftView;
}

export interface StoredLeague {
  league_id: string;
  name: string;
  season: string;
  /** Sleeper's `pre_draft` / `drafting` / `in_season` / `complete`; null
   *  for a mock draft or a config written before it was recorded. */
  status: string | null;
  /** Which service the league is read from. Always written by the backend;
   *  a config saved before Yahoo existed reads as Sleeper. */
  platform: Platform;
}

/** How far the Yahoo connection has got. The secret is never handed back —
 *  `configured` is the only thing the UI is told about it. */
export interface YahooStatus {
  /** A client id and secret have been saved. */
  configured: boolean;
  /** A refresh token is in the keychain, so Yahoo can be called. */
  connected: boolean;
  /** The redirect URI the saved app has to be registered with. */
  redirect: string;
  /** Whose Yahoo account it is, once connected. */
  account: string | null;
}

/** What `yahoo_begin_connect` hands back: where to send the user, and the
 *  state string `yahoo_finish_connect` has to be given back with the code. */
export interface YahooConnectStart {
  authorize_url: string;
  state: string;
  redirect: string;
}

export interface AppConfig {
  my_user_id: string | null;
  active_league_id: string | null;
  leagues: StoredLeague[];
}

export type Position = string;

// ---------- phone & second screen (see COMPANION-API.md) ----------

/** What kind of thing is paired: the trimmed phone page, or a second desktop
 *  app running in follower mode. */
export type DeviceKind = "phone" | "desktop";

/** One paired client, as the host lists it. */
export interface CompanionDevice {
  device_id: string;
  name: string;
  kind: DeviceKind;
  paired_at_ms: number;
  last_seen_ms: number;
  connected: boolean;
}

/** The host's own view of the companion server. */
export interface CompanionStatus {
  enabled: boolean;
  /** `http://<ip>:<port>/` — what the QR encodes. Empty while off. */
  url: string;
  /** The same server over Tailscale, when the Mac is on a tailnet. */
  tailscale_url?: string | null;
  /** The six digits a client pairs with. Rotates on revoke. */
  code: string;
  port: number;
  /** How the host signs its own messages in the shared chat. */
  host_name: string;
  devices: CompanionDevice[];
}

/** Who said a line in the shared thread. */
export interface SharedChatDevice {
  name: string;
  kind: DeviceKind;
}

/** One line of the shared thread. The assistant's entry carries the device
 *  that asked, not the host. */
export interface SharedChatEntry {
  id: string;
  at_ms: number;
  device: SharedChatDevice;
  role: "user" | "assistant";
  text: string;
  cost_usd: number | null;
  error: string | null;
}

/** The whole thread for one screen of one league. */
export interface SharedChatThread {
  league_id: string;
  screen: string;
  /** True while a question is being answered; nobody else may ask. */
  busy: boolean;
  entries: SharedChatEntry[];
}

/** What `GET /api/config` hands a follower — never keys, tokens or budget. */
export interface RemoteConfig {
  active_league_id: string | null;
  leagues: StoredLeague[];
  my_user_id: string | null;
  host_name: string;
  platform: Platform;
}

/** What this app remembers about the host it follows. */
export interface FollowRecord {
  /** Origin only, no trailing slash: `http://192.168.1.5:7878`. */
  url: string;
  token: string;
  host_name: string;
  /** The id the host gave this device when it paired. Sent back on the next
   *  pair so the host replaces the entry instead of listing "This Mac 2".
   *  Absent in records written before this was kept. */
  device_id?: string;
}

/** Everything the Diagnostics dialog shows, and everything "Copy diagnostics"
 *  puts on the clipboard. Mirrors `Diagnostics` in
 *  `src-tauri/src/commands_diag.rs`.
 *
 *  Deliberately carries no pairing code and no token: the whole point of it is
 *  that it can be pasted into a chat window. */
export interface Diagnostics {
  app_version: string;
  /** `macos aarch64` on the desktop; on a follower, who it is following. */
  platform: string;
  league_id: string | null;
  league_name: string | null;
  draft_id: string | null;
  /** Which service the league on screen is read from. */
  platform_name: string | null;
  polling: boolean;
  poll: PollHealth | null;
  companion_enabled: boolean;
  companion_devices: number;
  /** Null when this copy of the app has no log of its own — a follower, or
   *  the browser preview. The dialog hides the log actions then. */
  log_path: string | null;
  /** "debug" or "info": what the Verbose logging checkbox shows. */
  log_level: string;
  log_tail: string[];
}

/** Accounts returned by the current Sleeper league, independent of draft order. */
export interface SleeperMember {
  user_id: string;
  display_name: string | null;
  draft_slot: number | null;
  is_current: boolean;
}
