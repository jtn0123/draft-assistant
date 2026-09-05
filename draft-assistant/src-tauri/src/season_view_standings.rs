//! The standings-and-odds section of the season view.
//!
//! This is the expensive one: every remaining week of every roster is solved
//! for an optimal lineup, then the rest of the schedule is simulated for
//! playoff odds. Nothing here can move because a touchdown was scored, which
//! is why the live poller hands back a cached copy instead of rebuilding it.

use crate::engine::LoadedLeague;
use crate::season_api::{matchup_for, opponent_of, Matchup, Roster};
use crate::season_engine::LoadedSeason;
use crate::season_lineup::weekly_lineup_outlook;
use crate::season_lookup::Lookup;
use crate::season_odds::{self, ScheduledGame, StandingsRow, TeamSeason};
use crate::season_spread;
use crate::season_view_matchup::{current_lineup, live_score};
use std::collections::HashMap;

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

    // Where each NFL team's game stands. Empty before kickoff, which is what
    // makes the live pricing below reproduce the pregame numbers exactly.
    let remaining = crate::season_live::remaining_by_team(&season.scores);

    // This week's mean and spread for one roster, priced off its games rather
    // than off its projection alone. The lineup is the same best-available one
    // `weekly_lineup_outlook` solves for the same week, so with no game
    // started this returns exactly what the outlook already said.
    // What a starter is still worth this week. A player already ruled out is
    // worth nothing more, exactly as `candidates_for` scores him in every
    // other solve.
    let still_to_come = |id: &str| {
        if sidelined(id) {
            0.0
        } else {
            weekly.get_or_zero(id, week)
        }
    };
    let live_week = |roster: &Roster| -> Option<(f64, f64)> {
        let banked = matchup_for(&season.matchups, roster.roster_id)?;
        // The lineup this roster actually has set, not the one it should
        // have. Banked points are read off the matchup by player, and solving
        // for the optimal lineup first meant a 30-point player left on the
        // bench was picked into the lineup and then credited with every one
        // of those points: the odds banked a score nobody had.
        let lineup = current_lineup(loaded, banked.starter_ids(), &still_to_come);
        let live_of = |id: &str| live_score(Some(banked), &remaining, &team_of, id);
        let starters = season_spread::live_starters(&lineup, &position_of, &team_of, &live_of);
        Some((
            season_spread::total_points(&starters),
            season_spread::team_sigma(&starters),
        ))
    };

    // What each roster's record already accounts for. Sleeper's standings lag
    // the scoreboard by hours on a Monday night: every game is final and
    // `settings.wins` still has not moved. Counting the finished week off the
    // matchups fills that gap; see `finished_week_result`.
    let just_finished = if current_week_is_over(season) {
        finished_week_result(season)
    } else {
        HashMap::new()
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
            let mut weekly_projection: Vec<(u32, f64)> =
                outlook.iter().map(|w| (w.week, w.points)).collect();
            let mut weekly_sigma: Vec<(u32, f64)> =
                outlook.iter().map(|w| (w.week, w.sigma)).collect();
            // The week being played is priced off where its games actually
            // are, not off the projection it started on: banked points plus
            // whatever share of each starter's game is still to come, with
            // only that share carrying any spread. Without this the playoff
            // odds redrew the whole in-progress week from projections and
            // noise every tick, so a team fifty points up with every game
            // final was still simulated as a coin flip.
            // Skipped outright before kickoff: with nothing on the scoreboard
            // there is nothing to price off, and not solving the lineup again
            // is also the plainest guarantee that a Saturday reading is
            // untouched by any of this.
            if first_open == week && !remaining.is_empty() {
                if let Some((points, sigma)) = live_week(r) {
                    replace_week(&mut weekly_projection, week, points);
                    replace_week(&mut weekly_sigma, week, sigma);
                }
            }
            let counted = just_finished.get(&r.roster_id).copied();
            let result = counted.filter(|_| games_counted(r) < week);
            TeamSeason {
                roster_id: r.roster_id,
                wins: r.settings.wins + u32::from(result == Some(WeekResult::Won)),
                losses: r.settings.losses + u32::from(result == Some(WeekResult::Lost)),
                ties: r.settings.ties + u32::from(result == Some(WeekResult::Tied)),
                points_for: r.settings.points_for() + points_this_week(season, r, result.is_some()),
                weekly_projection,
                weekly_sigma,
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

    let mut rows = season_odds::standings(
        &teams,
        &schedule,
        playoff_teams,
        team_name,
        my_roster_id,
        seed,
    );
    // Past the last regular week the schedule is empty and the simulation has
    // nothing to run, so every percentage it hands back is a flat 1.0 or 0.0.
    // Show the state those numbers actually describe instead of dressing the
    // standings up as a forecast that is certain of itself.
    if week > last_regular {
        for row in &mut rows {
            row.playoff_status = Some(season_odds::playoff_status(row.seed, playoff_teams));
        }
    }
    rows
}

/// How the week just played came out for one roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeekResult {
    Won,
    Lost,
    Tied,
}

/// How many games each roster's record already accounts for. A week counts
/// once it has a win, a loss or a tie against it.
fn games_counted(roster: &Roster) -> u32 {
    roster.settings.wins + roster.settings.losses + roster.settings.ties
}

/// Who won this week, read off the matchup scores rather than the standings.
///
/// Sleeper posts a roster's win hours after the last whistle. In between, the
/// week simply vanished: the simulation had skipped it because every game was
/// final, and the standings had not yet counted it, so a Monday-night win
/// showed up nowhere at all and the playoff odds quietly priced the season as
/// one game shorter than it is.
fn finished_week_result(season: &LoadedSeason) -> HashMap<u32, WeekResult> {
    let mut out = HashMap::new();
    for mine in season.matchups.iter() {
        let Some(theirs) = opponent_of(&season.matchups, mine) else {
            continue;
        };
        let result = match mine.scored().total_cmp(&theirs.scored()) {
            std::cmp::Ordering::Greater => WeekResult::Won,
            std::cmp::Ordering::Less => WeekResult::Lost,
            std::cmp::Ordering::Equal => WeekResult::Tied,
        };
        out.insert(mine.roster_id, result);
    }
    out
}

/// This week's points for a roster, when they are not in its total yet.
fn points_this_week(season: &LoadedSeason, roster: &Roster, uncounted: bool) -> f64 {
    if !uncounted {
        return 0.0;
    }
    matchup_for(&season.matchups, roster.roster_id).map_or(0.0, Matchup::scored)
}

/// Overwrite one week's entry in a (week, value) list, leaving the rest alone.
fn replace_week(weeks: &mut [(u32, f64)], week: u32, value: f64) {
    for entry in weeks.iter_mut().filter(|(w, _)| *w == week) {
        entry.1 = value;
    }
}
