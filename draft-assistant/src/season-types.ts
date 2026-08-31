// Mirrors the Rust SeasonView structs (serde snake_case).

import type { LeagueSummary } from "./types";

export interface SeasonHeader {
  opponent_name: string | null;
  my_projected: number;
  opp_projected: number;
  /** 0..1 */
  win_odds: number;
  /** 0..1 */
  playoff_odds: number;
  locks_in_ms: number | null;
}

export interface MatchupRow {
  slot: string;
  my_player_id: string | null;
  my_name: string;
  my_team: string | null;
  my_points: number;
  opp_player_id: string | null;
  opp_name: string;
  opp_team: string | null;
  opp_points: number;
  margin: number;
}

export interface MatchupView {
  my_name: string;
  opp_name: string;
  /** Manager pictures for the two teams, when they have one. */
  my_avatar: string | null;
  opp_avatar: string | null;
  /** What my best lineup would score. */
  my_projected: number;
  opp_projected: number;
  /** My best lineup, slot by slot, against their set one. */
  rows: MatchupRow[];
  /** The lineup I actually have set, same comparison. */
  set_rows: MatchupRow[];
  /** What the lineup I have set would score. */
  set_projected: number;
}

export interface LineupCall {
  slot: string;
  player_in: string;
  player_in_id: string;
  player_in_team: string | null;
  player_out: string;
  player_out_id: string;
  gain: number;
  why: string;
}

export interface WaiverTarget {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  gain_points: number;
  gain_fraction: number;
  suggested_bid: number | null;
  rivals: number;
}

export interface StandingsRow {
  roster_id: number;
  seed: number;
  name: string;
  record: string;
  wins: number;
  losses: number;
  ties: number;
  points_for: number;
  projected_points: number;
  /** 0..1 */
  playoff_odds: number;
  is_mine: boolean;
}

export type PlayState = "pre" | "playing" | "done";
export type GameState = "pre" | "live" | "final";

export interface GameChip {
  player_id: string;
  name: string;
  slot: string;
  team: string | null;
  points: number;
  is_mine: boolean;
  state: PlayState;
}

export interface LiveGame {
  game_id: string;
  away: string;
  home: string;
  away_score: number | null;
  home_score: number | null;
  state: GameState;
  status: string;
  kickoff_ms: number;
  flag: string | null;
  /** Broadcaster — "CBS", "NBC/Peacock", "Netflix". Null when unpublished. */
  channel: string | null;
  chips: GameChip[];
}

export interface KickoffWindow {
  kickoff_ms: number;
  my_starters: number;
  games: LiveGame[];
}

export interface LiveTotals {
  my_playing: number;
  my_pre: number;
  my_done: number;
  my_live_points: number;
  opp_live_points: number;
}

export interface LiveSection {
  games: LiveGame[];
  windows: KickoffWindow[];
  totals: LiveTotals;
  next_kickoff_ms: number | null;
  /** NFL teams idle this week; empty when no schedule has loaded. */
  bye_teams: string[];
}

export interface RosterRow {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  /** "Start" | "Bench" | "Bye" */
  role: string;
  points: number;
  /** Projected points this week (0 on a bye). */
  projected: number;
}

export interface TradeIdea {
  roster_id: number;
  partner: string;
  get_id: string;
  get_name: string;
  get_position: string;
  give_id: string;
  give_name: string;
  give_position: string;
  my_edge: number;
  their_edge: number;
  note: string;
}

export interface TradeSide {
  roster_id: number;
  team: string;
  gets: string[];
}

export interface TradeDone {
  transaction_id: string;
  /** Epoch milliseconds. */
  at: number;
  sides: TradeSide[];
  involves_me: boolean;
  /** Accepted, but still inside the league's review window. */
  pending: boolean;
}

export interface ActivityItem {
  kind: string;
  text: string;
  created: number;
  /** The team the move belongs to, for its manager's picture. */
  roster_id: number | null;
  /** The players involved, so the row can show their faces. */
  player_ids: string[];
}

export interface LastSeasonRow {
  place: number;
  name: string;
  record: string;
  points: number;
  tag: string | null;
  is_mine: boolean;
}

export interface TrendPoint {
  /** Seconds since epoch. */
  at: number;
  week: number;
  /** Mean best-lineup points per remaining week. */
  strength: number;
}

export interface TeamSeries {
  roster_id: number;
  name: string;
  is_mine: boolean;
  points: TrendPoint[];
}

export interface TrendChange {
  at: number;
  week: number;
  roster_id: number;
  team: string;
  is_mine: boolean;
  /** Points per week gained or lost. */
  delta: number;
  /** Up to three explanations, biggest first. */
  reasons: string[];
}

export interface TrendsView {
  series: TeamSeries[];
  changes: TrendChange[];
}

export interface SourceStatus {
  /** Epoch seconds of the last successful fetch; 0 when it has never worked. */
  last_success_secs: number;
  /** Why the latest attempt failed; null when the latest attempt worked. */
  error: string | null;
}

/** The three feeds the live poll depends on, tracked one by one. */
export interface SourceHealth {
  matchups: SourceStatus;
  scores: SourceStatus;
  rosters: SourceStatus;
}

export interface SeasonHealth {
  fetched_at: number;
  warnings: string[];
  /** Optional: views cached before per-source tracking existed have none. */
  sources?: SourceHealth;
}

export interface SeasonView {
  schema_version: string;
  generated_at: number;
  league: LeagueSummary;
  week: number;
  season: string;
  my_roster_id: number | null;
  header: SeasonHeader;
  matchup: MatchupView | null;
  calls: LineupCall[];
  points_on_table: number;
  waivers: WaiverTarget[];
  waiver_budget_left: number | null;
  /** The league's full FAAB budget; null when it uses waiver priority. */
  waiver_budget_total: number | null;
  standings: StandingsRow[];
  live: LiveSection;
  roster: RosterRow[];
  trades: TradeIdea[];
  /** Completed trades this week and last, both sides named. */
  recent_trades: TradeDone[];
  activity: ActivityItem[];
  last_season: LastSeasonRow[];
  trends: TrendsView;
  /** roster_id -> the manager's avatar reference. */
  team_avatars: Record<string, string>;
  data_health: SeasonHealth;
  /**
   * Epoch seconds the standings/waivers/trades analysis was built. The live
   * poll reuses it for minutes at a time. Optional: older views have none.
   */
  analysis_as_of_secs?: number;
}

/** Which side panel tab the season screen is showing. */
export type SeasonTab = "Standings" | "Games" | "My team" | "Trends" | "League" | "Last season";

export const SEASON_TABS: SeasonTab[] = [
  "Standings",
  "Games",
  "My team",
  "Trends",
  "League",
  "Last season",
];
