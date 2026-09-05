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

#[derive(Debug, PartialEq)]
struct Args {
    league_id: String,
    max_week: u32,
    out_path: Option<String>,
}

const USAGE: &str = "usage: backtest <league_id> [--weeks N] [--out calib.json]";
/// Every regular-season week plus the playoffs, which is "no limit" here.
const ALL_WEEKS: u32 = 18;

/// The command line, read without touching the process or the environment, so
/// what the flags mean is a thing a test can ask rather than a thing only a
/// run can show.
fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Option<Args> {
    let mut args = args.into_iter();
    let mut league_id = None;
    let mut max_week = ALL_WEEKS;
    let mut out_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--weeks" => {
                max_week = args
                    .next()
                    .and_then(|w| w.parse().ok())
                    .unwrap_or(ALL_WEEKS)
            }
            "--out" => out_path = args.next(),
            _ => league_id = Some(arg),
        }
    }
    Some(Args {
        league_id: league_id?,
        max_week,
        out_path,
    })
}

fn parse_args() -> Args {
    parse_args_from(std::env::args().skip(1)).unwrap_or_else(|| {
        eprintln!("{USAGE}");
        std::process::exit(2);
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Option<Args> {
        parse_args_from(words.iter().map(|w| w.to_string()))
    }

    /// Without a league there is nothing to replay, and the binary says so
    /// rather than starting a run against an empty id.
    #[test]
    fn a_command_line_with_no_league_is_no_command_line_at_all() {
        assert_eq!(args(&[]), None);
        assert_eq!(args(&["--weeks", "4"]), None);
    }

    #[test]
    fn the_first_bare_word_is_the_league_and_the_flags_are_optional() {
        let parsed = args(&["123"]).expect("a league is enough");
        assert_eq!(parsed.league_id, "123");
        assert_eq!(parsed.max_week, ALL_WEEKS);
        assert_eq!(parsed.out_path, None);
    }

    #[test]
    fn the_flags_are_read_wherever_they_appear() {
        let parsed = args(&["--out", "calib.json", "123", "--weeks", "6"]).expect("parsed");
        assert_eq!(parsed.league_id, "123");
        assert_eq!(parsed.max_week, 6);
        assert_eq!(parsed.out_path.as_deref(), Some("calib.json"));
    }

    /// A typo in the week count used to be read as the league id, and the run
    /// then failed much later with "league not found". It falls back to the
    /// whole season instead, and the league is still the league.
    #[test]
    fn a_week_count_that_is_not_a_number_falls_back_to_the_whole_season() {
        let parsed = args(&["123", "--weeks", "lots"]).expect("parsed");
        assert_eq!(parsed.league_id, "123");
        assert_eq!(parsed.max_week, ALL_WEEKS);
    }

    fn meta(position: &str, team: &str) -> PlayerMeta {
        PlayerMeta {
            full_name: None,
            first_name: None,
            last_name: None,
            position: Some(position.to_string()),
            team: Some(team.to_string()),
            fantasy_positions: None,
            injury_status: None,
            years_exp: None,
            age: None,
        }
    }

    fn roster() -> HashMap<String, PlayerMeta> {
        HashMap::from([
            ("4034".to_string(), meta("RB", "TEN")),
            ("6794".to_string(), meta("WR", "MIN")),
        ])
    }

    /// Sleeper files a defence under its team abbreviation rather than a
    /// numeric id, so it is absent from the player dictionary entirely. Read
    /// as "unknown position" it dropped out of every lineup the backtest
    /// scored.
    #[test]
    fn a_team_defence_is_recovered_from_its_key_rather_than_the_dictionary() {
        let players = roster();
        assert_eq!(position_of(&players, "4034").as_deref(), Some("RB"));
        assert_eq!(position_of(&players, "DET").as_deref(), Some("DEF"));
        // A defence *is* its NFL team, so it stacks with its own kicker.
        assert_eq!(team_of(&players, "DET").as_deref(), Some("DET"));
        assert_eq!(team_of(&players, "4034").as_deref(), Some("TEN"));
        // A numeric id nobody has heard of is still nobody.
        assert_eq!(position_of(&players, "99999"), None);
        assert_eq!(team_of(&players, "99999"), None);
    }

    fn matchup(roster_id: u32, matchup_id: Option<u32>, points: f64) -> Matchup {
        Matchup {
            roster_id,
            matchup_id,
            points,
            custom_points: None,
            starters: Some(vec!["4034".to_string(), "0".to_string(), "DET".to_string()]),
            players: Some(vec![
                "4034".to_string(),
                "6794".to_string(),
                "DET".to_string(),
            ]),
            players_points: Some(HashMap::from([
                ("4034".to_string(), 21.0),
                ("6794".to_string(), 3.0),
            ])),
        }
    }

    /// "0" is Sleeper's empty slot. Counted as a starter it added a
    /// zero-point player to every lineup and dragged the projection down.
    #[test]
    fn an_empty_lineup_slot_is_not_a_starter() {
        let players = roster();
        let projections: WeekProjections =
            HashMap::from([("4034".to_string(), 14.5), ("DET".to_string(), 7.0)]);
        let starters = set_starters(&matchup(1, Some(9), 100.0), &projections, &players);
        assert_eq!(starters.len(), 2, "the '0' slot is not a player");
        assert_eq!(starters[0].position, "RB");
        assert_eq!(starters[0].points, 14.5);
        // A week is scored before it is played, so all of it is unsettled.
        assert_eq!(starters[0].uncertain, 14.5);
        assert_eq!(starters[1].position, "DEF");
    }

    /// A bye week has a matchup entry with no `matchup_id`, and pairing on a
    /// `None` would have matched every bye in the league to every other.
    #[test]
    fn only_entries_sharing_a_matchup_id_are_paired_and_byes_are_left_out() {
        let week = vec![
            matchup(1, Some(9), 101.0),
            matchup(2, Some(9), 99.0),
            matchup(3, None, 88.0),
            matchup(4, None, 77.0),
        ];
        let paired = pairs(&week);
        assert_eq!(paired.len(), 1);
        assert_eq!((paired[0].0.roster_id, paired[0].1.roster_id), (1, 2));
    }

    #[test]
    fn a_game_row_carries_both_sides_of_the_result_it_was_asked_about() {
        let players = roster();
        let projections: WeekProjections =
            HashMap::from([("4034".to_string(), 14.5), ("DET".to_string(), 7.0)]);
        let a = matchup(1, Some(9), 101.0);
        let b = matchup(2, Some(9), 99.0);
        let sa = set_starters(&a, &projections, &players);
        let sb = set_starters(&b, &projections, &players);
        let row = game_row(5, (&a, &sa), (&b, &sb));
        assert_eq!((row.week, row.roster_a, row.roster_b), (5, 1, 2));
        assert_eq!(row.actual_a, 101.0);
        assert_eq!(row.actual_b, 99.0);
        assert_eq!(row.projected_a, 21.5);
        assert_eq!(row.projected_b, 21.5);
        // Evenly matched projections are a coin flip, whatever happened.
        assert!((row.p_a - 0.5).abs() < 1e-3, "p_a was {}", row.p_a);
        assert!(row.sigma > 0.0);
    }

    /// The per-position fit needs both halves of a player's week. A starter
    /// with a projection but no result, or a result but no projection, is
    /// skipped rather than counted as a miss of the whole projection.
    #[test]
    fn only_starters_with_both_a_projection_and_a_result_reach_the_spread_fit() {
        let players = roster();
        let projections: WeekProjections =
            HashMap::from([("4034".to_string(), 14.5), ("DET".to_string(), 7.0)]);
        let mut out = Season::default();
        collect_weeks(
            &mut out,
            &[matchup(1, Some(9), 101.0)],
            &projections,
            &players,
        );
        // "4034" has both; "DET" was projected but never scored; "0" is not a
        // player at all.
        assert_eq!(out.weeks.len(), 1);
        assert_eq!(out.weeks[0].position, "RB");
        assert_eq!(out.weeks[0].projected, 14.5);
        assert_eq!(out.weeks[0].actual, 21.0);
    }
}
