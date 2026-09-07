//! The week turning over underneath a running poller, and what happens when
//! the reload that follows fails.
//!
//! Split out of `season_loop.rs`, which was at the line cap.

use super::SeasonEngine;
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::season_engine::week_watch::ROLLOVER_FAILED;
use crate::season_engine::LoadedSeason;
use tokio::sync::Mutex;

/// Put a failed rollover on the badge and in the log.
///
/// Both paths used to fail silently: the poller returned an empty tick and
/// the Refresh button fell through to re-fetching the old week, so from
/// Tuesday morning the screen scored a finished week with nothing anywhere
/// saying the new one had been tried and lost. The warning replaces any
/// earlier one of its kind, so the reason on the badge is the latest.
pub(super) async fn note_failure(
    season_ref: &Mutex<Option<LoadedSeason>>,
    league_id: &str,
    showing: u32,
    wanted: u32,
    error: &str,
) {
    crate::applog::warn(format!(
        "season week rollover to {wanted} failed, still showing week {showing}: {error}{}",
        crate::applog::context(&[("league", league_id)])
    ));
    let mut season = season_ref.lock().await;
    let Some(season) = season.as_mut() else {
        return;
    };
    season.warnings.retain(|w| !w.starts_with(ROLLOVER_FAILED));
    season.warnings.push(format!(
        "{ROLLOVER_FAILED}: still showing week {showing} ({error})"
    ));
}

/// Reload the whole season for a week that has just turned over, replacing
/// what the poller was watching. `Ok(false)` when the league changed
/// underneath the load, so there is nothing to emit this tick; `Err` when the
/// load itself failed, which the caller says out loud rather than swallowing.
///
/// The load runs with nothing locked, exactly like the live fetch: it is
/// fifteen matchup requests and can take seconds, and holding the season
/// across it would stall every command in the app.
pub async fn reload_for_week<E: SeasonEngine>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    season_ref: &Mutex<Option<LoadedSeason>>,
    config_ref: &Mutex<AppConfig>,
    league_id: &str,
) -> Result<bool, String> {
    let league = {
        let loaded = loaded_ref.lock().await;
        match loaded.as_ref() {
            Some(l) if l.league.league_id == league_id => l.league.clone(),
            _ => return Ok(false),
        }
    };
    let my_user_id = config_ref.lock().await.my_user_id.clone();
    let mut fresh = engine
        .load_season(&league, my_user_id.as_deref(), false)
        .await?;
    // Checked again on the way back in: the load ran unlocked, and writing
    // this would otherwise file one league's rosters under another's.
    //
    // The league is copied out rather than held, because recording the Trends
    // snapshot below reads that file, diffs it and writes it back.
    let mine = {
        let loaded = loaded_ref.lock().await;
        match loaded.as_ref() {
            Some(l) if l.league.league_id == league_id => l.clone(),
            _ => return Ok(false),
        }
    };
    // `Engine::load_season` hands back an empty history, because the file it
    // lives in is the command layer's business. The user-driven load fills it
    // in; the automatic rollover did not, so every Tuesday morning the Trends
    // tab silently emptied itself and the week just finished was never
    // recorded at all.
    fresh.history = std::sync::Arc::new(engine.record_history(&mine, &fresh).await);
    // A new season as far as the poller's caches are concerned, whatever the
    // loader stamped it with.
    fresh.restamp();
    *season_ref.lock().await = Some(fresh);
    Ok(true)
}

/// Re-pull the live slice for the Refresh button, rolling the week over first
/// when the NFL has moved on.
///
/// The rollover check is the same one the poller makes, on the same
/// `current_week` call, because Refresh used to skip it entirely: the live
/// slice is asked for by week, so from Tuesday morning the button re-fetched
/// the finished week forever and the only way to see the new one was to close
/// the league and open it again.
pub async fn refresh_or_roll<E: SeasonEngine>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    season_ref: &Mutex<Option<LoadedSeason>>,
    config_ref: &Mutex<AppConfig>,
) -> Result<(), String> {
    let league_id = {
        let loaded = loaded_ref.lock().await;
        loaded
            .as_ref()
            .ok_or("no league loaded")?
            .league
            .league_id
            .clone()
    };
    let watching = {
        let season = season_ref.lock().await;
        let season = season.as_ref().ok_or("season data not loaded")?;
        (season.season, season.week)
    };
    if let Ok(week) = engine.current_week().await {
        if week != watching.1 {
            match reload_for_week(engine, loaded_ref, season_ref, config_ref, &league_id).await {
                Ok(true) => return Ok(()),
                // The league changed under the load: the live fetch below
                // notices the same thing and refuses to apply.
                Ok(false) => {}
                // Said on the badge and in the log, and then the week we do
                // have is refreshed: last week's live slice beats nothing.
                Err(error) => {
                    note_failure(season_ref, &league_id, watching.1, week, &error).await;
                }
            }
        }
    }
    // Fetched with nothing locked: three requests with retries behind them can
    // run for tens of seconds, and everything else that needs the season would
    // be waiting the whole time.
    let fetched = engine.fetch_live(&league_id, watching.0, watching.1).await;
    // Locks in the usual order, loaded then season. The league is checked
    // again here because the fetch ran unlocked: folding this week's scoring
    // into whatever season happens to be loaded now would show one league's
    // live points on another league's screen.
    let loaded = loaded_ref.lock().await;
    if loaded.as_ref().map(|l| l.league.league_id.as_str()) != Some(league_id.as_str()) {
        return Err("the league changed while this was loading \u{2014} try again".to_string());
    }
    let mut season = season_ref.lock().await;
    let season = season.as_mut().ok_or("season data not loaded")?;
    fetched.apply(season, now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug: a rollover that failed left no line in the log and nothing on
    /// the badge, on either path.
    #[tokio::test]
    async fn a_failed_rollover_is_logged_once_and_put_on_the_badge_once() {
        let season = Mutex::new(Some(LoadedSeason {
            week: 3,
            ..LoadedSeason::default()
        }));
        let capture = crate::applog::Capture::start();

        note_failure(&season, "42", 3, 4, "request failed").await;
        note_failure(&season, "42", 3, 4, "timed out").await;

        assert!(
            capture.saw("WARN season week rollover to 4 failed, still showing week 3: request failed league=42"),
            "{:?}",
            capture.lines()
        );
        let warnings = season.lock().await.as_ref().unwrap().warnings.clone();
        assert_eq!(
            warnings,
            vec![format!(
                "{ROLLOVER_FAILED}: still showing week 3 (timed out)"
            )],
            "one warning, carrying the latest reason"
        );
    }
}
