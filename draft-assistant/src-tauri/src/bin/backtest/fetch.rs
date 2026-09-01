//! Everything the backtest has to ask Sleeper for, and the disk cache that
//! means it only asks once.
//!
//! The app's own `cache` module is deliberately crate-private and is built
//! around a TTL, which is the wrong shape here: a finished season does not
//! change, so a hit is always good and the only thing worth spending is the
//! first fetch. This is that cache — a filename, a JSON body, no envelope.

use draft_assistant_lib::scoring::{base_points, bonus_points};
use draft_assistant_lib::season_api::{Matchup, SeasonEndpoints};
use draft_assistant_lib::sleeper::{League, PlayerMeta, SleeperClient};
use draft_assistant_lib::sleeper_error::to_message;
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
pub async fn cached<T, F, Fut>(name: &str, fetch: F) -> Result<T, String>
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

pub async fn league(client: &SleeperClient, league_id: &str) -> Result<League, String> {
    client.league(league_id).await.map_err(to_message)
}

pub async fn matchups(
    client: &SleeperClient,
    league_id: &str,
    week: u32,
) -> Result<Vec<Matchup>, String> {
    client.matchups(league_id, week).await.map_err(to_message)
}

/// The ~14 MB player dictionary, parsed once and cached parsed.
pub async fn players(client: &SleeperClient) -> Result<HashMap<String, PlayerMeta>, String> {
    cached("players", || async {
        let bytes = client.players_bytes().await.map_err(to_message)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("players: {e}"))
    })
    .await
}

/// A player's projected points for one week under this league's scoring.
pub type WeekProjections = HashMap<String, f64>;

/// One week's projections, re-scored with the league's own rules — the same
/// `base_points` + `bonus_points` the app projects with.
pub async fn week_projections(
    client: &SleeperClient,
    season: u32,
    week: u32,
    scoring: &HashMap<String, f64>,
) -> Result<WeekProjections, String> {
    let rows = cached(&format!("proj-{season}-w{week}"), || async {
        client
            .weekly_projections(season, week)
            .await
            .map_err(to_message)
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
