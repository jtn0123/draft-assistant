//! The standings-and-odds section of the season view.
//!
//! This is the expensive one: every remaining week of every roster is solved
//! for an optimal lineup, then the rest of the schedule is simulated for
//! playoff odds. Nothing here can move because a touchdown was scored, which
//! is why the live poller hands back a cached copy instead of rebuilding it.

use crate::engine::LoadedLeague;
use crate::season_engine::LoadedSeason;
use crate::season_lineup::weekly_lineup_outlook;
use crate::season_lookup::Lookup;
use crate::season_odds::{self, ScheduledGame, StandingsRow, TeamSeason};

/// True when every NFL game the scoreboard knows of this week has finished.
///
/// A scoreboard that says nothing about the week is not evidence that it is
/// decided, so it counts as unfinished — simulating a week that is already in
/// the books costs a little accuracy, whereas skipping one that is not hands
/// back false certainty.
fn current_week_is_over(season: &LoadedSeason) -> bool {
    let mut games = season
        .scores
        .iter()
        .filter(|game| game.week == Some(season.week))
        .peekable();
    games.peek().is_some() && games.all(|game| game.meta().is_some_and(|meta| meta.is_over))
}

/// Project every roster's rest of season, then simulate the remaining schedule.
pub fn standings_rows(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    lookup: &Lookup,
    my_roster_id: Option<u32>,
    team_name: &impl Fn(u32) -> String,
) -> Vec<StandingsRow> {
    let rules = &loaded.roster_rules;
    let weekly = &loaded.weekly_points;
    let week = season.week;
    let position_of = |id: &str| lookup.position(id);
    let team_of = |id: &str| lookup.team(id);
    let sidelined = |id: &str| lookup.is_sidelined(id);
    let last_regular = loaded.league.last_regular_week();
    // Where the rest of the season starts. This week counts as still to play
    // until its games are actually over: dropping it left the last regular
    // week with nothing scheduled, and a simulation with no games left reads
    // the standings off as certainty — every roster a flat 100% or 0% while
    // the games deciding them were still being played.
    let first_open = if current_week_is_over(season) {
        week + 1
    } else {
        week
    };

    let teams: Vec<TeamSeason> = season
        .rosters
        .iter()
        .map(|r| {
            // Positions are resolved once per roster here, not once per
            // (roster, week) — the answer cannot change from week to week.
            let outlook = weekly_lineup_outlook(
                rules,
                r.player_ids(),
                &position_of,
                &team_of,
                &sidelined,
                weekly,
                first_open..=last_regular,
            );
            TeamSeason {
                roster_id: r.roster_id,
                wins: r.settings.wins,
                losses: r.settings.losses,
                ties: r.settings.ties,
                points_for: r.settings.points_for(),
                weekly_projection: outlook.iter().map(|w| (w.week, w.points)).collect(),
                weekly_sigma: outlook.iter().map(|w| (w.week, w.sigma)).collect(),
            }
        })
        .collect();

    let schedule: Vec<ScheduledGame> = season
        .schedule
        .iter()
        .filter(|(w, _)| *w >= first_open)
        .flat_map(|(w, pairs)| {
            pairs.iter().map(move |(home, away)| ScheduledGame {
                week: *w,
                home: *home,
                away: *away,
            })
        })
        .collect();

    let playoff_teams = loaded.league.settings.playoff_teams.unwrap_or(6);
    // Seeded from league identity plus how far the season has progressed, so
    // odds stay put between refreshes but do move as results land.
    let seed = season
        .rosters
        .iter()
        .map(|r| r.settings.wins as u64 * 31 + r.settings.fpts as u64)
        .fold(week as u64, |acc, x| {
            acc.wrapping_mul(1_000_003).wrapping_add(x)
        });

    season_odds::standings(
        &teams,
        &schedule,
        playoff_teams,
        team_name,
        my_roster_id,
        seed,
    )
}
