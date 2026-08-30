//! Replay a finished season through the win-probability model and print how
//! well it did: calibration, spread, and what a week around a projection
//! really looks like per position.
//!
//! Usage: backtest <league_id> [--weeks N] [--out calib.json]
//!
//! Everything fetched is cached under `$TMPDIR/draft-assistant-backtest`
//! (or `DRAFT_ASSISTANT_BACKTEST_DIR`), so a second run is instant. The
//! weekly projection files are the slow part — about 3 MB each.

use draft_assistant_lib::backtest::{
    best_sigma_scale, calibrate, log_loss_at, position_fits, win_probability, Game, PlayerWeek,
};
use draft_assistant_lib::lineup::{best_lineup, Candidate, Starter};
use draft_assistant_lib::matchup::Teams;
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::scoring::{base_points, bonus_points};
use draft_assistant_lib::season::Matchup;
use draft_assistant_lib::sleeper::{PlayerMeta, SleeperClient};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    std::env::var("DRAFT_ASSISTANT_BACKTEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("draft-assistant-backtest"))
}

/// Fetch once, then read from disk forever: last season does not change.
async fn cached<T, F, Fut>(name: &str, fetch: F) -> Result<T, String>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let dir = cache_dir();
    let path = dir.join(format!("{name}.json"));
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(value) = serde_json::from_str(&text) {
            return Ok(value);
        }
    }
    let value = fetch().await?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(value)
}

/// A player's projected points for one week under this league's scoring.
type WeekProjections = HashMap<String, f64>;

async fn week_projections(
    client: &SleeperClient,
    season: u32,
    week: u32,
    scoring: &HashMap<String, f64>,
) -> Result<WeekProjections, String> {
    let rows = cached(&format!("proj-{season}-w{week}"), || {
        client.weekly_projections(season, week)
    })
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let stats = row.stats.as_ref()?;
            Some((
                row.player_id.clone(),
                base_points(stats, scoring) + bonus_points(&[stats], scoring),
            ))
        })
        .collect())
}

fn position_of(players: &HashMap<String, PlayerMeta>, id: &str) -> String {
    players
        .get(id)
        .and_then(|p| p.position.clone())
        .or_else(|| {
            // Team defenses are keyed by team abbreviation, not an id.
            (id.len() <= 3 && id.chars().all(|c| c.is_ascii_alphabetic()))
                .then(|| "DEF".to_string())
        })
        .unwrap_or_else(|| "FLEX".to_string())
}

fn name_of(players: &HashMap<String, PlayerMeta>, id: &str) -> String {
    players
        .get(id)
        .and_then(|p| p.full_name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// The lineup as set, scored on projections — the side of the matchup the
/// model sees before kickoff.
fn set_starters(
    m: &Matchup,
    projections: &WeekProjections,
    players: &HashMap<String, PlayerMeta>,
) -> Vec<Starter> {
    m.starters
        .iter()
        .filter(|id| *id != "0")
        .map(|id| Starter {
            slot: position_of(players, id),
            player_id: id.clone(),
            name: name_of(players, id),
            position: position_of(players, id),
            points: projections.get(id).copied().unwrap_or(0.0),
            injury: None,
        })
        .collect()
}

/// The best lineup the roster could have set that week — what the app shows
/// for my own side.
fn best_starters(
    m: &Matchup,
    projections: &WeekProjections,
    players: &HashMap<String, PlayerMeta>,
    rules: &RosterRules,
) -> Vec<Starter> {
    let candidates: Vec<Candidate> = m
        .players
        .iter()
        .map(|id| Candidate {
            player_id: id.clone(),
            name: name_of(players, id),
            position: position_of(players, id),
            points: projections.get(id).copied().unwrap_or(0.0),
            bye_week: None,
            injury: None,
        })
        .collect();
    best_lineup(&candidates, rules).1
}

fn teams_of(players: &HashMap<String, PlayerMeta>) -> Teams {
    players
        .iter()
        .filter_map(|(id, p)| Some((id.clone(), p.team.clone()?)))
        .collect()
}

struct Season {
    games: Vec<Game>,
    /// The same games with both sides at their best lineup: how much of the
    /// edge the app claims is really "and you set a perfect lineup".
    best_games: Vec<Game>,
    weeks: Vec<PlayerWeek>,
}

async fn replay(client: &SleeperClient, league_id: &str, max_week: u32) -> Result<Season, String> {
    let league = cached(&format!("league-{league_id}"), || client.league(league_id)).await?;
    let season: u32 = league
        .season
        .parse()
        .map_err(|_| "bad season".to_string())?;
    let players: HashMap<String, PlayerMeta> = cached("players", || client.players()).await?;
    let teams = teams_of(&players);
    let rules = RosterRules::new(&league.roster_positions);
    let last = max_week.min(league.settings.playoff_week_start.saturating_sub(1));
    eprintln!("{} ({season}) — weeks 1..{last}", league.name);
    let mut out = Season {
        games: Vec::new(),
        best_games: Vec::new(),
        weeks: Vec::new(),
    };
    for week in 1..=last {
        let matchups: Vec<Matchup> = cached(&format!("matchups-{league_id}-w{week}"), || {
            client.matchups(league_id, week)
        })
        .await?;
        // A week nobody scored in is a week that was never played.
        if matchups.iter().all(|m| m.points <= 0.0) {
            eprintln!("week {week}: not played, stopping");
            break;
        }
        let projections = week_projections(client, season, week, &league.scoring_settings).await?;
        for m in &matchups {
            for id in m.starters.iter().filter(|id| *id != "0") {
                let (Some(projected), Some(actual)) =
                    (projections.get(id), m.players_points.get(id))
                else {
                    continue;
                };
                out.weeks.push(PlayerWeek {
                    position: position_of(&players, id),
                    projected: *projected,
                    actual: *actual,
                });
            }
        }
        let mut paired: Vec<(&Matchup, &Matchup)> = Vec::new();
        for (i, a) in matchups.iter().enumerate() {
            for b in matchups.iter().skip(i + 1) {
                if a.matchup_id.is_some() && a.matchup_id == b.matchup_id {
                    paired.push((a, b));
                }
            }
        }
        eprintln!("week {week}: {} games", paired.len());
        for (a, b) in paired {
            let (sa, sb) = (
                set_starters(a, &projections, &players),
                set_starters(b, &projections, &players),
            );
            let (p, sigma) = win_probability(&sa, &sb, &teams);
            out.games.push(Game {
                week,
                roster_a: a.roster_id,
                roster_b: b.roster_id,
                p_a: p,
                projected_a: sa.iter().map(|s| s.points).sum(),
                projected_b: sb.iter().map(|s| s.points).sum(),
                actual_a: a.points,
                actual_b: b.points,
                sigma,
            });
            let (ba, bb) = (
                best_starters(a, &projections, &players, &rules),
                best_starters(b, &projections, &players, &rules),
            );
            let (bp, bsigma) = win_probability(&ba, &bb, &teams);
            out.best_games.push(Game {
                week,
                roster_a: a.roster_id,
                roster_b: b.roster_id,
                p_a: bp,
                projected_a: ba.iter().map(|s| s.points).sum(),
                projected_b: bb.iter().map(|s| s.points).sum(),
                actual_a: a.points,
                actual_b: b.points,
                sigma: bsigma,
            });
        }
    }
    Ok(out)
}

fn report(label: &str, games: &[Game]) -> serde_json::Value {
    let c = calibrate(games);
    println!("\n{label}: {} games", c.games);
    println!(
        "  favourite won {:.0}% · brier {:.3} (coin flip .250) · log loss {:.3} (.693)",
        c.accuracy * 100.0,
        c.brier,
        c.log_loss
    );
    println!(
        "  margin error {:.2} of the spread (1.00 = a normal season; under 1 \
         is the hedge for upsets) · bias {:+.2} · score error {:.1} pts",
        c.z_sd, c.z_mean, c.score_mae
    );
    println!("  band          games   said   won");
    for b in &c.buckets {
        println!(
            "  {:.0}–{:.0}%{:>12}  {:5.0}% {:5.0}%",
            b.low * 100.0,
            b.high * 100.0,
            b.games,
            b.predicted * 100.0,
            b.actual * 100.0
        );
    }
    let (scale, loss) = best_sigma_scale(games);
    println!(
        "  best spread scale {:.2} → log loss {:.3} (from {:.3} at 1.00)",
        scale,
        loss,
        log_loss_at(games, 1.0)
    );
    let mut value = serde_json::to_value(&c).unwrap_or(serde_json::Value::Null);
    if let Some(map) = value.as_object_mut() {
        map.insert("best_sigma_scale".into(), scale.into());
        map.insert("log_loss_at_best_scale".into(), loss.into());
    }
    value
}

/// Fit the spread scale on the first half of the season and grade it on the
/// second: a scale that only helps the games it was fitted to is noise.
fn holdout(games: &[Game]) {
    let Some(last) = games.iter().map(|g| g.week).max() else {
        return;
    };
    let cut = last / 2;
    let (fit, test): (Vec<Game>, Vec<Game>) = games.iter().cloned().partition(|g| g.week <= cut);
    if fit.is_empty() || test.is_empty() {
        return;
    }
    let (scale, _) = best_sigma_scale(&fit);
    println!(
        "\nholdout: scale {:.2} fitted on weeks 1–{cut} ({} games) → weeks {}–{last} ({} games) \
         log loss {:.3}, against {:.3} unscaled",
        scale,
        fit.len(),
        cut + 1,
        test.len(),
        log_loss_at(&test, scale),
        log_loss_at(&test, 1.0)
    );
}

#[tokio::main]
async fn main() {
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
    let client = SleeperClient::new();
    let season = match replay(&client, &league_id, max_week).await {
        Ok(season) => season,
        Err(error) => {
            eprintln!("backtest failed: {error}");
            std::process::exit(1);
        }
    };
    let as_set = report("lineups as set (both sides)", &season.games);
    let as_best = report("best lineups (both sides)", &season.best_games);
    holdout(&season.games);
    println!("\nper position, starters projected 4+ points");
    println!("  pos   weeks   bias    real cv   model cv");
    let fits = position_fits(&season.weeks);
    for f in &fits {
        println!(
            "  {:<5} {:>6}   {:.2}     {:.2}      {:.2}",
            f.position, f.games, f.bias, f.cv, f.model_cv
        );
    }
    if let Some(path) = out_path {
        let value = serde_json::json!({
            "league_id": league_id,
            "as_set": as_set,
            "as_best": as_best,
            "positions": fits,
        });
        match serde_json::to_string_pretty(&value).map_err(|e| e.to_string()) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    eprintln!("could not write {path}: {error}");
                }
            }
            Err(error) => eprintln!("could not encode report: {error}"),
        }
    }
}
