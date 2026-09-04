//! Replay a finished season through the win-probability model and print how
//! well it did: calibration, spread, and what a week around a projection
//! really looks like per position.
//!
//! Usage: `cargo run --bin backtest -- <league_id> [--weeks N] [--out calib.json]`
//!
//! Everything fetched is cached under `$TMPDIR/draft-assistant-backtest` (or
//! `DRAFT_ASSISTANT_BACKTEST_DIR`), so a second run is instant. The weekly
//! projection files are the slow part — about 3 MB each.
//!
//! The arithmetic lives in `draft_assistant_lib::backtest`; this file is only
//! the fetching, the shaping into that module's rows, and the printing.

// The crate root of a bin resolves `mod x;` next to itself, and every
// `src/bin/*.rs` is its own bin target — so these live in a subdirectory and
// are pointed at explicitly.
#[path = "backtest/fetch.rs"]
mod fetch;
#[path = "backtest/report.rs"]
mod report;

use draft_assistant_lib::backtest::{win_probability, Game, PlayerWeek};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::season_api::Matchup;
use draft_assistant_lib::season_lineup::{optimal_lineup, Candidate};
use draft_assistant_lib::season_spread::{starters_of, Starter};
use draft_assistant_lib::sleeper::{PlayerMeta, SleeperClient};
use fetch::{cached, players, week_projections, WeekProjections};
use std::collections::HashMap;

/// A player's position, with team defenses recovered from their key: Sleeper
/// files them under the team abbreviation rather than a numeric id.
fn position_of(players: &HashMap<String, PlayerMeta>, id: &str) -> Option<String> {
    players
        .get(id)
        .and_then(|p| p.position.clone())
        .or_else(|| {
            (id.len() <= 3 && id.chars().all(|c| c.is_ascii_alphabetic()))
                .then(|| "DEF".to_string())
        })
}

fn team_of(players: &HashMap<String, PlayerMeta>, id: &str) -> Option<String> {
    players
        .get(id)
        .and_then(|p| p.team.clone())
        // A defense *is* its NFL team, so it stacks with its own kicker.
        .or_else(|| (position_of(players, id).as_deref() == Some("DEF")).then(|| id.to_string()))
}

/// The lineup as set, scored on projections — the side of the matchup the
/// model sees before kickoff. Sleeper writes "0" for an empty slot.
fn set_starters(
    m: &Matchup,
    projections: &WeekProjections,
    players: &HashMap<String, PlayerMeta>,
) -> Vec<Starter> {
    m.starter_ids()
        .iter()
        .filter(|id| id.as_str() != "0")
        .map(|id| {
            let points = projections.get(id).copied().unwrap_or(0.0);
            Starter {
                position: position_of(players, id).unwrap_or_default(),
                team: team_of(players, id),
                points,
                // The backtest scores a week before it is played, so every
                // point of every projection is still unsettled.
                uncertain: points,
            }
        })
        .collect()
}

/// The best lineup the roster could have set that week — what the app shows
/// for my own side, through the same `optimal_lineup` the screen calls.
fn best_starters(
    m: &Matchup,
    projections: &WeekProjections,
    players: &HashMap<String, PlayerMeta>,
    rules: &RosterRules,
) -> Vec<Starter> {
    let candidates: Vec<Candidate> = m
        .players
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|id| {
            Some(Candidate {
                player_id: id.clone(),
                position: position_of(players, id)?,
                points: projections.get(id).copied().unwrap_or(0.0),
            })
        })
        .collect();
    let lineup = optimal_lineup(rules, &candidates);
    starters_of(
        &lineup,
        &|id: &str| position_of(players, id),
        &|id: &str| team_of(players, id),
    )
}

/// Both sides of one game, already reduced to starters, as a scored row.
fn game_row(week: u32, a: (&Matchup, &[Starter]), b: (&Matchup, &[Starter])) -> Game {
    let (p, sigma) = win_probability(a.1, b.1);
    Game {
        week,
        roster_a: a.0.roster_id,
        roster_b: b.0.roster_id,
        p_a: p,
        projected_a: a.1.iter().map(|s| s.points).sum(),
        projected_b: b.1.iter().map(|s| s.points).sum(),
        actual_a: a.0.scored(),
        actual_b: b.0.scored(),
        sigma,
    }
}

/// Both entries of every played head-to-head, paired on `matchup_id`.
fn pairs(matchups: &[Matchup]) -> Vec<(&Matchup, &Matchup)> {
    let mut paired = Vec::new();
    for (i, a) in matchups.iter().enumerate() {
        for b in matchups.iter().skip(i + 1) {
            if a.matchup_id.is_some() && a.matchup_id == b.matchup_id {
                paired.push((a, b));
            }
        }
    }
    paired
}

#[derive(Default)]
struct Season {
    games: Vec<Game>,
    /// The same games with both sides at their best lineup: how much of the
    /// edge the app claims is really "and you set a perfect lineup".
    best_games: Vec<Game>,
    weeks: Vec<PlayerWeek>,
}

/// Every started player's week, for the per-position spread fit.
fn collect_weeks(
    out: &mut Season,
    matchups: &[Matchup],
    projections: &WeekProjections,
    players: &HashMap<String, PlayerMeta>,
) {
    for m in matchups {
        for id in m.starter_ids().iter().filter(|id| id.as_str() != "0") {
            let (Some(projected), Some(actual)) = (projections.get(id), m.points_for(id)) else {
                continue;
            };
            out.weeks.push(PlayerWeek {
                position: position_of(players, id).unwrap_or_default(),
                projected: *projected,
                actual,
            });
        }
    }
}

async fn replay(client: &SleeperClient, league_id: &str, max_week: u32) -> Result<Season, String> {
    let league = cached(&format!("league-{league_id}"), || async {
        fetch::league(client, league_id).await
    })
    .await?;
    let season: u32 = league
        .season
        .parse()
        .map_err(|_| "bad season".to_string())?;
    let players = players(client).await?;
    let rules = RosterRules::new(&league.roster_positions);
    let last = max_week.min(league.last_regular_week());
    eprintln!("{} ({season}) — weeks 1..{last}", league.name);

    let mut out = Season::default();
    for week in 1..=last {
        let matchups = cached(&format!("matchups-{league_id}-w{week}"), || async {
            fetch::matchups(client, league_id, week).await
        })
        .await?;
        // A week nobody scored in is a week that was never played.
        if matchups.iter().all(|m| m.scored() <= 0.0) {
            eprintln!("week {week}: not played, stopping");
            break;
        }
        let projections = week_projections(client, season, week, &league.scoring_settings).await?;
        collect_weeks(&mut out, &matchups, &projections, &players);
        let paired = pairs(&matchups);
        eprintln!("week {week}: {} games", paired.len());
        for (a, b) in paired {
            let sa = set_starters(a, &projections, &players);
            let sb = set_starters(b, &projections, &players);
            out.games.push(game_row(week, (a, &sa), (b, &sb)));
            let ba = best_starters(a, &projections, &players, &rules);
            let bb = best_starters(b, &projections, &players, &rules);
            out.best_games.push(game_row(week, (a, &ba), (b, &bb)));
        }
    }
    Ok(out)
}

struct Args {
    league_id: String,
    max_week: u32,
    out_path: Option<String>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut league_id = None;
    let mut max_week = 18u32;
    let mut out_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--weeks" => max_week = args.next().and_then(|w| w.parse().ok()).unwrap_or(18),
            "--out" => out_path = args.next(),
            _ => league_id = Some(arg),
        }
    }
    let Some(league_id) = league_id else {
        eprintln!("usage: backtest <league_id> [--weeks N] [--out calib.json]");
        std::process::exit(2);
    };
    Args {
        league_id,
        max_week,
        out_path,
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let client = SleeperClient::new();
    let season = match replay(&client, &args.league_id, args.max_week).await {
        Ok(season) => season,
        Err(error) => {
            eprintln!("backtest failed: {error}");
            std::process::exit(1);
        }
    };
    let as_set = report::report("lineups as set (both sides)", &season.games);
    let as_best = report::report("best lineups (both sides)", &season.best_games);
    report::holdout(&season.games);
    let fits = report::positions(&season.weeks);
    if let Some(path) = args.out_path {
        report::write_json(&path, &args.league_id, as_set, as_best, &fits);
    }
}
