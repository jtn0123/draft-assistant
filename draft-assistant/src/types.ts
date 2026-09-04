// Mirrors the Rust DraftView structs (serde snake_case).

/** What an imported projections file says about one player. Ranks only: the
 *  export is half-PPR and this league is not, so its points do not compare. */
export interface SecondOpinion {
  positional_rank: number;
  overall_rank: number;
  /** Short label for whoever published it — "Clay", "FantasyPros". */
  source: string;
}

export interface BoardPlayer {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  bye_week: number | null;
  points: number;
  bonus_points: number;
  vorp: number;
  tier: number;
  position_rank: number;
  overall_rank: number;
  adp: number | null;
  injury_status: string | null;
  sleeper_pts_ppr: number | null;
  /** null unless a projections CSV has been imported and matched this player. */
  second_opinion: SecondOpinion | null;
}

// AvailablePlayer flattens BoardPlayer + survival_next.
export interface AvailablePlayer extends BoardPlayer {
  survival_next: number | null;
}

export interface RosterEntry {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  pick_no: number;
  round: number;
  /** Kept from last season rather than drafted tonight. */
  is_keeper: boolean;
}

export interface TeamRoster {
  slot: number;
  display_name: string | null;
  players: RosterEntry[];
  open_starters: [string, number][];
}

export interface DraftStatus {
  draft_id: string;
  status: string;
  teams: number;
  rounds: number;
  pick_timer: number | null;
  current_pick: number;
  current_round: number;
  on_clock_slot: number;
  on_clock_name: string | null;
  my_slot: number | null;
  is_my_pick: boolean;
  picks_until_mine: number | null;
  my_next_picks: number[];
  total_picks_made: number;
  manual_picks_active: boolean;
  /** Epoch ms when the current pick's timer expires; null when no clock runs. */
  clock_deadline_ms: number | null;
  /**
   * Picks the plain snake gets wrong — traded away, or moved by third-round
   * reversal: pick number (as a string key) -> the slot that makes it. Empty
   * in an ordinary league.
   */
  pick_slot_overrides: Record<string, number>;
  /** Picks already in the book as keepers: nobody's turn, ever. */
  keeper_picks: number[];
}

export interface TierAlert {
  position: string;
  tier: number;
  players_left: number;
}

export interface Recommendation {
  mode: string;
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  points: number;
  vorp: number;
  tier: number;
  adp: number | null;
  survival_next: number | null;
  score: number;
  reasons: string[];
}

export interface RecentPick {
  pick_no: number;
  round: number;
  slot: number;
  slot_name: string | null;
  player_id: string;
  name: string;
  position: string;
  team: string | null;
}

/** A position taken `count` times in the last `window` picks. */
export interface PositionRun {
  position: string;
  count: number;
  window: number;
}

export interface LeagueSummary {
  league_id: string;
  name: string;
  season: string;
  /** Which service this league is read from. */
  platform: Platform;
  total_rosters: number;
  roster_positions: string[];
  draftable_positions: string[];
  scoring_settings: Record<string, number>;
}

export interface DataHealth {
  players_fetched_at: number;
  projections_fetched_at: number;
  weekly_fetched_at: number;
  board_size: number;
  warnings: string[];
  poll_last_success_at: number | null;
  poll_consecutive_failures: number;
  poll_last_error: string | null;
  /** When the imported second-opinion CSV was last read, epoch seconds. */
  second_opinion_loaded_at: number | null;
}

export interface PollHealth {
  last_success_at: number | null;
  consecutive_failures: number;
  last_error: string | null;
}

/** What a pick in one round of this draft has been worth, in points over
 *  replacement — the median of what that round actually took. */
export interface PickPrice {
  round: number;
  points: number;
  example: string | null;
}

export interface DraftView {
  schema_version: string;
  generated_at: number;
  league: LeagueSummary;
  draft: DraftStatus;
  my_roster: TeamRoster | null;
  rosters: TeamRoster[];
  available: AvailablePlayer[];
  tier_alerts: TierAlert[];
  position_run: PositionRun | null;
  recommendations: Recommendation[];
  recent_picks: RecentPick[];
  replacement_baselines: Record<string, number>;
  replacement_demand: Record<string, number>;
  /** Optional: a fixture captured before pick pricing existed has none. */
  pick_prices?: PickPrice[];
  data_health: DataHealth;
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

/** The services a league can be read from. */
export type Platform = "sleeper" | "yahoo";
