//! The season side of a load: the NFL calendar, the schedule, the league's
//! records, transactions, the trending list and last season. All best
//! effort — the draft needs none of it, and a mock draft has no league to
//! ask — so every failure is a warning, never an error.
//!
//! Fanned out: the schedule alone is fourteen requests, last season's
//! transactions eighteen more, and one slow week must not stall a load. Run
//! in series this took thirteen seconds against the real league.

use crate::engine::Engine;
use crate::history::LeagueHistory;
use crate::sleeper::{
    League, LeagueRoster, Matchup, NflState, SleeperClient, Transaction, TrendingPlayer,
};
use std::collections::HashMap;
use tokio::task::JoinSet;

/// Last season does not change; a week is plenty.
const HISTORY_TTL_SECS: u64 = 7 * 24 * 3600;

pub struct SeasonContext {
    pub nfl_state: Option<NflState>,
    pub trending: Vec<TrendingPlayer>,
    pub matchups: Vec<Matchup>,
    pub league_rosters: Vec<LeagueRoster>,
    pub past_matchups: Vec<(u32, Vec<Matchup>)>,
    pub transactions: Vec<(u32, Vec<Transaction>)>,
    pub schedule: Vec<(u32, Vec<Matchup>)>,
    pub history: Option<LeagueHistory>,
}

/// One request per week, all at once; empty weeks dropped, failures named.
async fn per_week<T, F, Fut>(
    weeks: impl Iterator<Item = u32>,
    what: &str,
    warnings: &mut Vec<String>,
    fetch: F,
) -> Vec<(u32, Vec<T>)>
where
    T: Send + 'static,
    F: Fn(u32) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<T>, String>> + Send + 'static,
{
    let mut set = JoinSet::new();
    for w in weeks {
        let fut = fetch(w);
        set.spawn(async move { (w, fut.await) });
    }
    let mut out = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((w, Ok(rows))) if !rows.is_empty() => out.push((w, rows)),
            Ok((_, Ok(_))) => {}
            Ok((w, Err(error))) => warnings.push(format!("week {w} {what} unavailable ({error})")),
            Err(error) => warnings.push(format!("{what} task failed ({error})")),
        }
    }
    out.sort_by_key(|(w, _)| *w);
    out
}

impl Engine {
    pub(crate) async fn season_context(
        &self,
        league: &League,
        user_names: &HashMap<String, String>,
        warnings: &mut Vec<String>,
    ) -> SeasonContext {
        let client: SleeperClient = self.client.clone();
        let league_id = league.league_id.clone();
        let is_league = !user_names.is_empty();

        let (nfl_state, trending, league_rosters) =
            tokio::join!(client.nfl_state(), client.trending_adds(), async {
                if is_league {
                    client.league_rosters(&league_id).await
                } else {
                    Ok(Vec::new())
                }
            });
        let nfl_state = match nfl_state {
            Ok(state) => Some(state),
            Err(error) => {
                warnings.push(format!(
                    "NFL week unavailable ({error}); planning for week 1"
                ));
                None
            }
        };
        let week = nfl_state
            .as_ref()
            .and_then(NflState::upcoming_week)
            .unwrap_or(1);
        let trending = trending.unwrap_or_else(|error| {
            warnings.push(format!("trending adds unavailable ({error})"));
            Vec::new()
        });
        let league_rosters = league_rosters.unwrap_or_else(|error| {
            warnings.push(format!("league records unavailable ({error})"));
            Vec::new()
        });

        let (schedule, transactions) = if is_league {
            let last_regular = league
                .settings
                .playoff_week_start
                .saturating_sub(1)
                .clamp(1, 18);
            let (c1, l1) = (client.clone(), league_id.clone());
            let (c2, l2) = (client.clone(), league_id.clone());
            let mut w1 = Vec::new();
            let mut w2 = Vec::new();
            let (schedule, transactions) = tokio::join!(
                per_week(1..=last_regular, "matchups", &mut w1, move |w| {
                    let (c, l) = (c1.clone(), l1.clone());
                    async move { c.matchups(&l, w).await }
                }),
                per_week(1..=week, "transactions", &mut w2, move |w| {
                    let (c, l) = (c2.clone(), l2.clone());
                    async move { c.transactions(&l, w).await }
                })
            );
            warnings.extend(w1);
            warnings.extend(w2);
            (schedule, transactions)
        } else {
            (Vec::new(), Vec::new())
        };
        let matchups = schedule
            .iter()
            .find(|(w, _)| *w == week)
            .map(|(_, m)| m.clone())
            .unwrap_or_default();
        // Completed weeks only exist once the regular season is under way.
        let in_season = nfl_state
            .as_ref()
            .is_some_and(|s| s.season_type == "regular");
        let past_matchups: Vec<(u32, Vec<Matchup>)> = if in_season {
            schedule
                .iter()
                .filter(|(w, _)| *w < week)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let history = match league.previous_league_id.as_deref() {
            Some(prev) if is_league => self.league_history(prev, warnings).await,
            _ => None,
        };
        SeasonContext {
            nfl_state,
            trending,
            matchups,
            league_rosters,
            past_matchups,
            transactions,
            schedule,
            history,
        }
    }

    /// Last season, cached for a week: it is not going to change.
    async fn league_history(
        &self,
        prev: &str,
        warnings: &mut Vec<String>,
    ) -> Option<LeagueHistory> {
        let name = format!("history_{prev}.json");
        if let Some((_, hit)) = self.read_cache::<LeagueHistory>(&name, HISTORY_TTL_SECS) {
            return Some(hit);
        }
        let client = self.client.clone();
        let prev_id = prev.to_string();
        let (rosters, users) = tokio::join!(client.league_rosters(prev), client.league_users(prev));
        let (rosters, users) = match (rosters, users) {
            (Ok(r), Ok(u)) => (r, u),
            (Err(e), _) | (_, Err(e)) => {
                warnings.push(format!("last season unavailable ({e})"));
                return None;
            }
        };
        let mut w = Vec::new();
        let transactions = per_week(1..=18, "last-season transactions", &mut w, move |week| {
            let (c, l) = (client.clone(), prev_id.clone());
            async move { c.transactions(&l, week).await }
        })
        .await;
        warnings.extend(w);
        let history = crate::history::build(prev, &rosters, &users, &transactions);
        let (_, warning) = self.write_cache(&name, &history);
        warnings.extend(warning);
        Some(history)
    }
}
