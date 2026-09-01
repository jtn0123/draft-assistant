// Mirrors the Rust DraftView structs (serde snake_case).

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

export interface StoredLeague {
  league_id: string;
  name: string;
  season: string;
}

export interface AppConfig {
  my_user_id: string | null;
  active_league_id: string | null;
  leagues: StoredLeague[];
}

export type Position = string;
