//! Keeping the season side fresh once the draft is over: the calendar, the
//! week's matchups and lineups, records, transactions and the trending list
//! change on a scale of hours, not the three-second beat of a live draft.

use crate::app::AppCore;
use crate::engine_season::SeasonContext;
use crate::log;
use crate::trade::TradeVerdict;
use crate::view::{build_view, DraftView};
use std::time::Duration;

/// How often the season side is re-read once the draft is over.
pub(crate) const SEASON_REFRESH: Duration = Duration::from_secs(30 * 60);
/// The poll's own beat once the draft is over: nothing changes by the second.
pub(crate) const SEASON_IDLE: Duration = Duration::from_secs(60);

impl AppCore {
    pub(crate) async fn draft_over(&self) -> bool {
        self.loaded
            .lock()
            .await
            .as_ref()
            .is_some_and(|l| l.draft.status == "complete")
    }

    /// Re-read everything `engine_season` fetches and rebuild the view. The
    /// board, picks and projections are left alone — `refresh_data` is the
    /// heavy reload for those.
    pub async fn refresh_season(&self) -> Result<DraftView, String> {
        let started = std::time::Instant::now();
        let mut guard = self.loaded.lock().await;
        let loaded = guard.as_mut().ok_or("no league loaded")?;
        let mut warnings = Vec::new();
        let SeasonContext {
            nfl_state,
            trending,
            matchups,
            league_rosters,
            past_matchups,
            transactions,
            schedule,
            history,
        } = self
            .engine
            .season_context(&loaded.league, &loaded.user_names, &mut warnings)
            .await;
        loaded.nfl_state = nfl_state;
        loaded.trending = trending;
        loaded.matchups = matchups;
        loaded.league_rosters = league_rosters;
        loaded.past_matchups = past_matchups;
        loaded.transactions = transactions;
        loaded.schedule = schedule;
        if history.is_some() {
            loaded.history = history;
        }
        // Injuries move daily and the player dictionary is where they live.
        // Once a day is plenty, and a failed fetch keeps yesterday's tags.
        let age = crate::engine::now_secs().saturating_sub(loaded.players_fetched_at);
        if age >= crate::engine::PLAYERS_TTL_SECS {
            match self.engine.players(true).await {
                Ok((at, meta, warning)) => {
                    let changed = crate::board::apply_player_meta(&mut loaded.board, &meta);
                    loaded.player_meta = meta;
                    loaded.players_fetched_at = at;
                    warnings.extend(warning);
                    log::info(format!("injuries refreshed: {changed} players changed"));
                }
                Err(error) => warnings.push(format!("injury refresh failed ({error})")),
            }
        }
        // Replace last time's season warnings rather than pile them up.
        loaded.warnings.retain(|w| !is_season_warning(w));
        loaded.warnings.extend(warnings.iter().cloned());
        log::warnings("refresh_season", &warnings);
        log::info(format!(
            "refresh_season in {:.1}s: week {:?}, {} matchups, {} records, {} past weeks, {} transaction weeks",
            started.elapsed().as_secs_f64(),
            loaded.nfl_state.as_ref().map(|s| s.week),
            loaded.matchups.len(),
            loaded.league_rosters.len(),
            loaded.past_matchups.len(),
            loaded.transactions.len(),
        ));
        let config = self.config.lock().await.clone();
        Ok(build_view(loaded, &config))
    }
}

impl AppCore {
    /// Price an offer against the current rosters. Read-only: nothing is
    /// sent anywhere — Sleeper has no API for that.
    pub async fn evaluate_trade(
        &self,
        partner_slot: u32,
        give: Vec<String>,
        get: Vec<String>,
    ) -> Result<TradeVerdict, String> {
        let guard = self.loaded.lock().await;
        let loaded = guard.as_ref().ok_or("no league loaded")?;
        let config = self.config.lock().await.clone();
        let view = build_view(loaded, &config);
        let my_slot = view
            .draft
            .my_slot
            .ok_or("set your Sleeper username first")?;
        let week = view.this_week.as_ref().map_or(1, |w| w.week);
        let offer = crate::trade::Offer {
            my_slot,
            partner_slot,
            give: &give,
            get: &get,
            week,
        };
        let verdict = crate::trade::evaluate(loaded, &view.rosters, &offer, &loaded.roster_rules)?;
        log::info(format!(
            "evaluate_trade with slot {partner_slot}: give {} get {} -> me {:+.0}, them {:+.0}",
            give.len(),
            get.len(),
            verdict.my_season_after - verdict.my_season_before,
            verdict.their_season_after - verdict.their_season_before
        ));
        Ok(verdict)
    }
}

fn is_season_warning(w: &str) -> bool {
    w.starts_with("NFL week unavailable")
        || w.starts_with("trending adds unavailable")
        || w.contains("matchups unavailable")
        || w.starts_with("league records unavailable")
        || w.contains("transactions unavailable")
        || w.starts_with("traded picks unavailable")
        || w.starts_with("injury refresh failed")
}
