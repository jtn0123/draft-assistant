// Shared fixtures for the two SeasonScreen test files. Split out when the
// single file crossed the repository's line cap, so both halves build the
// same view rather than drifting into two slightly different leagues.

import type { MatchupView, SeasonView, SourceHealth } from "../season-types";

/** A matchup whose set lineup is worse than the best one available — the
 *  state in which the header and the lineup panel used to disagree. */
export function matchup(): MatchupView {
  const row = {
    slot: "QB",
    my_player_id: "a",
    my_name: "Jalen Hurts",
    my_team: "PHI",
    my_points: 22.4,
    opp_player_id: "b",
    opp_name: "Baker Mayfield",
    opp_team: "TB",
    opp_points: 15.2,
    margin: 7.2,
  };
  return {
    my_name: "Trust the Process",
    opp_name: "punt_god",
    my_avatar: null,
    opp_avatar: null,
    my_projected: 122.4,
    opp_projected: 108.9,
    rows: [row],
    set_projected: 118.1,
    set_rows: [{ ...row, my_name: "Bryce Young", my_points: 18.1, margin: 2.9 }],
  };
}

// Grade item D8. The badge's whole job is to notice how long ago something
// happened, so every test here runs against a clock that is standing still:
// otherwise "5 seconds ago" is a race against the second hand, and the
// thresholds below could never be asserted at the boundary itself.
export const FROZEN = Date.parse("2026-09-13T17:00:00Z");
export const NOW = () => Math.floor(FROZEN / 1000);

export function fresh(): SourceHealth {
  return {
    matchups: { last_success_secs: NOW() - 5, error: null },
    scores: { last_success_secs: NOW() - 5, error: null },
    rosters: { last_success_secs: NOW() - 5, error: null },
  };
}

export function view(overrides: Partial<SeasonView> = {}): SeasonView {
  return {
    schema_version: "1.3",
    generated_at: 0,
    team_avatars: {},
    league: {
      league_id: "1",
      name: "Dynasty Warriors",
      season: "2026",
      platform: "sleeper",
      total_rosters: 12,
      roster_positions: ["QB", "RB", "BN"],
      draftable_positions: ["QB", "RB"],
      scoring_settings: {},
    },
    week: 3,
    season: "2026",
    my_roster_id: 1,
    header: {
      opponent_name: "punt_god",
      my_projected: 122.4,
      my_set_projected: 118.1,
      opp_projected: 108.9,
      win_odds_best: 0.62,
      win_odds_set: 0.55,
      playoff_odds: 0.88,
      playoff_status: null,
      locks_in_ms: null,
    },
    matchup: null,
    calls: [],
    points_on_table: 0,
    waivers: [],
    waiver_budget_left: 38,
    waiver_budget_total: 100,
    standings: [],
    live: {
      games: [],
      windows: [],
      totals: { my_playing: 0, my_pre: 0, my_done: 0, my_live_points: 0, opp_live_points: 0 },
      next_kickoff_ms: null,
      bye_teams: [],
    },
    roster: [],
    trades: [],
    recent_trades: [],
    activity: [],
    last_season: [],
    trends: { series: [], changes: [] },
    data_health: { fetched_at: NOW(), warnings: [], sources: fresh() },
    ...overrides,
  };
}

/** One of my starters, in whatever state his game is. */
export function myChip(state: "pre" | "playing" | "done", player_id = "a") {
  return {
    player_id,
    name: "Jalen Hurts",
    slot: "QB",
    team: "PHI",
    points: 22.4,
    is_mine: true,
    state,
  };
}

/** A game already under way, carrying one of my starters. */
export function liveGame(state: "pre" | "playing" | "done" = "playing", id = "phi-tb") {
  return {
    game_id: id,
    away: "PHI",
    home: "TB",
    away_score: 7,
    home_score: 3,
    state:
      state === "pre"
        ? ("pre" as const)
        : state === "done"
          ? ("final" as const)
          : ("live" as const),
    status: state === "playing" ? "Q2 08:14" : state === "done" ? "Final" : "",
    kickoff_ms: FROZEN - 3600_000,
    flag: null,
    channel: "FOX",
    chips: [myChip(state, id)],
  };
}

/** The view with every one of my starters already on the field. */
export function lockedView() {
  const base = view({ matchup: matchup() });
  return {
    ...base,
    calls: [],
    live: { ...base.live, games: [liveGame()] },
  };
}
