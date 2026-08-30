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
  /** Kept from last season rather than drafted tonight. */
  is_keeper: boolean;
}

export interface TeamRoster {
  slot: number;
  display_name: string | null;
  players: RosterEntry[];
  open_starters: [string, number][];
}

export interface Starter {
  slot: string;
  player_id: string;
  name: string;
  position: string;
  points: number;
  /** Sleeper's injury tag, when there is one. */
  injury: string | null;
}

/** A team's best lineup and what it projects to over the season. */
export interface TeamProjection {
  slot: number;
  display_name: string | null;
  /** Best lineup, season total, nobody ever on a bye. */
  full_strength: number;
  /** Week-by-week best lineup with byes honoured, summed. */
  season: number;
  starters: Starter[];
  /** Which week `week_points` is for. */
  week: number;
  /** Best lineup from that week's own projections; 0 with no rows. */
  week_points: number;
  week_starters: Starter[];
}

export interface LineupChange {
  slot: string;
  /** Who is set there now; null for an empty slot. */
  out: Starter | null;
  in_: Starter;
  gain: number;
}

export interface LineupCheck {
  set_points: number;
  best_points: number;
  changes: LineupChange[];
  empty_slots: string[];
  /** Set starters tagged but not sidelined (Questionable): check the inactives before kickoff. */
  questionable: Starter[];
}

export interface MatchupPreview {
  opponent_slot: number;
  opponent_name: string | null;
  my_points: number;
  opponent_points: number;
  margin: number;
  win_probability: number;
  my_starters: Starter[];
  opponent_starters: Starter[];
}

/** The week ahead: lineup check against Sleeper, and the matchup. */
export interface ThisWeek {
  week: number;
  lineup: LineupCheck | null;
  matchup: MatchupPreview | null;
}

export interface WaiverTarget {
  player_id: string;
  name: string;
  position: string;
  team: string | null;
  bye_week: number | null;
  points: number;
  /** Season points he adds to my bye-adjusted lineup total. */
  my_gain: number;
  /** Rivals he would lift by 5+ points: the competition for the claim. */
  rivals_helped: number;
  trending_adds: number | null;
  /** Why the gain: weeks he starts, byes he covers; "never starts". */
  reason: string;
  /** FAAB, from what winning claims cost last season; null without history. */
  suggested_bid: number | null;
}

export interface DropCandidate {
  player_id: string;
  name: string;
  position: string;
  points: number;
  /** Weeks he starts in my best lineup; 0 = never. */
  starts: number;
}

export interface WaiverBoard {
  targets: WaiverTarget[];
  drops: DropCandidate[];
}

export interface StandingRow {
  slot: number;
  display_name: string | null;
  wins: number;
  losses: number;
  ties: number;
  points_for: number;
  points_against: number;
}

export interface WeekResult {
  week: number;
  my_points: number;
  opponent_slot: number | null;
  opponent_name: string | null;
  opponent_points: number | null;
  won: boolean | null;
}

export interface PlayerTrend {
  player_id: string;
  name: string;
  position: string;
  games: number;
  projected: number;
  actual: number;
  delta_per_game: number;
}

/** Record, standings, results and projected-vs-actual, once a week has been played. */
export interface SeasonSoFar {
  through_week: number;
  standings: StandingRow[];
  my_results: WeekResult[];
  trends: PlayerTrend[];
}

export interface Activity {
  /** ms since the epoch. */
  at: number;
  week: number;
  kind: string;
  /** "complete", or "failed" for a waiver claim that lost. */
  status: string;
  teams: string[];
  adds: [string, string][];
  drops: [string, string][];
  picks: string[];
  bid: number | null;
}

/** A one-for-one swap that lifts both my season lineup and a rival's. */
export interface TradeIdea {
  partner_slot: number;
  partner_name: string | null;
  give_id: string;
  give: string;
  give_position: string;
  /** A second player going with `give`: a two-for-one. */
  also_give_id: string | null;
  also_give: string | null;
  also_give_position: string | null;
  get_id: string;
  get: string;
  get_position: string;
  my_gain: number;
  /** my_gain less what the best free agent at that position adds for nothing. */
  over_waiver: number;
  their_gain: number;
  /** Trades this manager made last season; null without history. */
  partner_trades: number | null;
}

/** Simulated rest of season: how often a team makes the playoffs. */
export interface TeamOdds {
  slot: number;
  display_name: string | null;
  /** 0..1 */
  playoff_odds: number;
  expected_wins: number;
  expected_points: number;
  /** Simulated seasons. */
  runs: number;
}

export interface BidStats {
  count: number;
  median: number;
  p75: number;
  max: number;
}

export interface ManagerProfile {
  user_id: string;
  display_name: string | null;
  trades: number;
  moves: number;
  faab_used: number;
  wins: number;
  losses: number;
  points_for: number;
}

/** Last season: who trades, who churns, what claims cost. */
export interface LeagueHistory {
  league_id: string;
  trades: number;
  claims: number;
  bids: BidStats;
  managers: ManagerProfile[];
}

/** An offer priced both ways: season (byes honoured) and this week. */
export interface TradeVerdict {
  partner_slot: number;
  partner_name: string | null;
  give: Starter[];
  get: Starter[];
  my_season_before: number;
  my_season_after: number;
  their_season_before: number;
  their_season_after: number;
  week: number;
  my_week_before: number;
  my_week_after: number;
  their_week_before: number;
  their_week_after: number;
  give_picks: PickPrice[];
  get_picks: PickPrice[];
}

/** What a round of the draft is worth, in points over replacement. */
export interface PickPrice {
  round: number;
  points: number;
  example: string | null;
}

/** One week where someone on my roster is on a bye. */
export interface ByeWeek {
  week: number;
  /** Who is out, in roster order. */
  out: string[];
  /** Best lineup that week, from season projections. */
  points: number;
  /** What the same lineup scores in a week with nobody out. */
  shortfall: number;
  /** Starting slots left empty that week. */
  empty_slots: string[];
}

export interface DraftStatus {
  draft_id: string;
  status: string;
  teams: number;
  rounds: number;
  pick_timer: number | null;
  /** Scheduled start, ms since the epoch. */
  start_time: number | null;
  /** When the current pick's clock runs out (ms since the epoch); live drafts with a timer only. */
  pick_deadline: number | null;
  current_pick: number;
  current_round: number;
  on_clock_slot: number;
  on_clock_name: string | null;
  my_slot: number | null;
  is_my_pick: boolean;
  picks_until_mine: number | null;
  my_next_picks: number[];
  /** A required slot is still empty and the picks are running out. */
  starter_alert: string | null;
  /** Picks that do not follow the snake because they were traded: pick
   *  number -> the slot whose manager makes it. Keys are strings (JSON). */
  traded_pick_slots: Record<string, number>;
  total_picks_made: number;
  manual_picks_active: boolean;
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

export interface DraftView {
  schema_version: string;
  generated_at: number;
  /// Strictly increasing per backend build; used to discard out-of-order updates.
  seq: number;
  league: LeagueSummary;
  draft: DraftStatus;
  my_roster: TeamRoster | null;
  rosters: TeamRoster[];
  available: AvailablePlayer[];
  tier_alerts: TierAlert[];
  position_run: string | null;
  recommendations: Recommendation[];
  recent_picks: RecentPick[];
  /** Every team, best projected season first. */
  projected_standings: TeamProjection[];
  this_week: ThisWeek | null;
  waivers: WaiverBoard | null;
  season: SeasonSoFar | null;
  activity: Activity[];
  trade_ideas: TradeIdea[];
  /** Empty without a schedule. Best odds first. */
  playoff_odds: TeamOdds[];
  history: LeagueHistory | null;
  /** My bye weeks, worst first. Empty without a roster. */
  bye_weeks: ByeWeek[];
  /** player_id -> projected points for weeks 1..17 (0 on a bye), rostered players and waiver targets. */
  player_weeks: Record<string, number[]>;
  /** Round by round, what a pick costs. Empty before the draft has picks. */
  pick_prices: PickPrice[];
  replacement_baselines: Record<string, number>;
  replacement_demand: Record<string, number>;
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

// ---------- Ask Claude ----------

/** A conversation line as sent to the backend. Panel-only notes are not sent. */
export interface ChatTurn {
  role: "you" | "claude" | "summary";
  text: string;
}

export interface ChatOptions {
  model: string;
  /** null = the CLI's default. */
  effort: string | null;
  fast: boolean;
  web_search: boolean;
}

export interface ChatUsage {
  model: string;
  input_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  output_tokens: number;
  /** Everything the model read on this call — the size of the thread. */
  context_tokens: number;
  web_searches: number;
  duration_ms: number;
  cost_usd: number | null;
  fast_mode: string | null;
  fast_mode_reason: string | null;
}

/** The draft moment an answer was written against. */
export interface ChatAsOf {
  pick: number;
  seq: number;
}

export interface ChatReply {
  answer: string;
  usage: ChatUsage;
  /** null for a compaction, which is about the conversation, not a pick. */
  as_of: ChatAsOf | null;
}

/** A line of a saved Ask Claude conversation (`chat/session.rs`). */
export interface ChatSessionTurn {
  role: "you" | "claude" | "summary" | "note";
  text: string;
  as_of_pick: number | null;
}

/** A saved conversation: one JSON file per session in the app's data dir. */
export interface ChatSession {
  id: string;
  draft_id: string;
  league_name: string;
  /** Unix seconds. */
  started_at: number;
  updated_at: number;
  /** The first question, clipped. */
  title: string;
  turns: ChatSessionTurn[];
  questions: number;
  cost_usd: number;
}

export interface ChatSessionSummary {
  id: string;
  title: string;
  started_at: number;
  updated_at: number;
  questions: number;
  cost_usd: number;
}
