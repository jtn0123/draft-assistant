//! Loading a Yahoo league into the shapes the rest of the app already reads.
//!
//! Four Yahoo calls — league settings, teams, draft results, the player pool —
//! and the three Sleeper ones the board is scored from. Yahoo publishes no
//! projections and no ADP, so the numbers stay Sleeper's; what Yahoo supplies
//! is the league, the rosters, the draft order and the picks. The bridge
//! between the two is [`crate::yahoo_crosswalk`].
//!
//! Caching follows `crate::projections`' policy exactly: a fresh cache is
//! served without a request, a fetch that fails falls back to whatever stale
//! copy is on disk with a warning attached, and only a failure with no cache
//! behind it takes the load down. Files are named `yahoo_<key>_<what>.json`
//! so one league's cache never shadows another's. The player pool has one
//! rule of its own — it is fetched a page at a time and resumes where a
//! throttled load stopped — and that lives in `crate::engine_yahoo_pool`.
//!
//! Draft results are the one thing that is never cached: they are the live
//! part, and a stale pick list is worse than none.

use crate::engine::{Engine, LoadedLeague};
use crate::engine_assemble::AssemblyParts;
use crate::sleeper::{Draft, DraftSettings, Pick};
use crate::yahoo::{YahooClient, YahooError};
use crate::yahoo_crosswalk::Crosswalk;
use crate::yahoo_map;
use crate::yahoo_types::{YahooDraftPick, YahooLeague, YahooPlayer, YahooTeam};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;

/// How long a league's settings and its team list are served from disk. Short:
/// the draft order and the draft status both live in them and both move.
pub(crate) const YAHOO_LEAGUE_TTL_SECS: u64 = 300;

/// The cache file for one league's copy of one resource.
///
/// The key is scrubbed rather than trusted: it reaches here from a paste box,
/// and a `/` in it would put the file somewhere else entirely.
pub fn cache_name(league_key: &str, what: &str) -> String {
    let safe: String = league_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("yahoo_{safe}_{what}.json")
}

impl Engine {
    /// One cached Yahoo read, with Sleeper's freshness and fallback rules.
    async fn yahoo_cached<T, F, Fut>(
        &self,
        name: &str,
        ttl: u64,
        force: bool,
        what: &str,
        fetch: F,
    ) -> Result<(T, Option<String>), String>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, YahooError>>,
    {
        if !force {
            if let Some((_, hit)) = self.read_cache_off_thread::<T>(name, ttl).await {
                return Ok((hit, None));
            }
        }
        let stale = self.read_cache_any_off_thread::<T>(name).await;
        match fetch().await {
            Ok(data) => {
                self.write_cache_off_thread(name, &data).await;
                Ok((data, None))
            }
            Err(error) => stale
                .map(|(at, data)| {
                    let age = crate::engine::now_secs().saturating_sub(at);
                    (
                        data,
                        Some(format!(
                            "Yahoo {what} refresh failed; using cache aged {}h ({error})",
                            age / 3600
                        )),
                    )
                })
                .ok_or_else(|| format!("Yahoo {what}: {error}")),
        }
    }

    /// The account's leagues, and the game key each one belongs to.
    ///
    /// Not cached: it is one call, and it is how a league is found in the
    /// first place — a stale answer would hide a league joined this morning.
    pub async fn yahoo_user_leagues(
        &self,
        client: &YahooClient,
    ) -> Result<Vec<YahooLeague>, String> {
        client
            .user_leagues(crate::yahoo::NFL)
            .await
            .map_err(|e| e.to_string())
    }

    /// The nickname Yahoo has for the logged-in manager, if any league's team
    /// list is already on disk. Never fetches: this answers a settings screen.
    pub fn yahoo_cached_account(&self, league_key: &str) -> Option<String> {
        let (_, teams) = self.read_cache_any::<Vec<YahooTeam>>(&cache_name(league_key, "teams"))?;
        current_login(&teams).map(|team| {
            team.managers
                .iter()
                .find(|manager| manager.is_current_login)
                .map(|manager| manager.nickname.clone())
                .unwrap_or_else(|| team.name.clone())
        })
    }

    /// Load a Yahoo league end-to-end and build its scored board.
    pub async fn load_yahoo_league(
        &self,
        client: &YahooClient,
        league_key: &str,
        force: bool,
    ) -> Result<LoadedLeague, String> {
        // The season decides which projections to ask for, so the league
        // settings have to land before the rest goes out.
        let (yahoo_league, league_warning) = self
            .yahoo_cached(
                &cache_name(league_key, "league"),
                YAHOO_LEAGUE_TTL_SECS,
                force,
                "league settings",
                || client.league(league_key),
            )
            .await?;
        let league = yahoo_map::league(&yahoo_league);
        let season: u32 = league
            .season
            .parse()
            .map_err(|_| format!("Yahoo league {league_key} reports no season"))?;

        // Everything else is independent: three Yahoo reads and the three
        // Sleeper ones the board is scored from, all in flight together.
        let teams_name = cache_name(league_key, "teams");
        let (teams, pool, results, sleeper) = tokio::join!(
            self.yahoo_cached(&teams_name, YAHOO_LEAGUE_TTL_SECS, force, "teams", || {
                client.league_teams(league_key)
            },),
            self.yahoo_pool(client, league_key, force),
            client.draft_results(league_key),
            self.sleeper_inputs(season, force),
        );
        let (teams, teams_warning): (Vec<YahooTeam>, _) = teams?;
        let (pool, pool_warning): (Vec<YahooPlayer>, _) = pool?;
        let (players, season_projections, weekly) = sleeper?;
        let (players_at, sleeper_players, players_warning) = players;
        let (projections_at, season_rows, projections_warning) = season_projections;
        let (weekly_at, weekly_rows, weekly_warning) = weekly;

        // A draft that has not started answers with an empty list rather than
        // an error, so only a real failure is worth reporting.
        let (results, poll_last_success_at, poll_consecutive_failures, poll_last_error) =
            match results {
                Ok(results) => (results, Some(crate::engine::now_secs()), 0, None),
                Err(error) => (Vec::new(), None, 1, Some(error.to_string())),
            };

        let mapped = yahoo_map::players(&pool);
        let crosswalk = crate::yahoo_crosswalk::build(&mapped, &sleeper_players);
        let api_picks = picks_for(&results, &teams, &pool, &crosswalk);

        let mut warnings = Vec::new();
        warnings.extend(league_warning);
        warnings.extend(teams_warning);
        warnings.extend(pool_warning);
        warnings.extend(players_warning);
        warnings.extend(projections_warning);
        warnings.extend(weekly_warning);
        warnings.extend(crosswalk.warning());
        // A stat this app cannot score is silently worth zero to every player,
        // which is invisible on the board and changes the ranking; saying so is
        // the difference between a board that is wrong and one that is wrong
        // and says why.
        warnings.extend(yahoo_map::unscored_stats_warning(&yahoo_league));

        self.finish_assembly(AssemblyParts {
            draft: draft_for(league_key, &yahoo_league, &teams),
            league,
            user_names: team_names(&teams),
            // Yahoo's team logos are not on the team resource this app reads,
            // and the draft screen only ever shows manager names.
            user_avatars: HashMap::new(),
            api_picks,
            // Yahoo has no traded-pick resource for a draft, so pick
            // ownership is the plain snake.
            traded_picks: Vec::new(),
            my_slot: current_login(&teams).and_then(|team| team.draft_position),
            // Keyed the way a pick names a player, so the poll tick can remap
            // a fresh pick list without re-indexing anything.
            yahoo_ids: crossed_ids(&crosswalk),
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players: (players_at, crosswalk.player_meta),
            season_projections: (projections_at, season_rows),
            weekly: (weekly_at, weekly_rows),
            warnings,
        })
    }

    /// The three Sleeper payloads a board is scored from, fetched together.
    #[allow(clippy::type_complexity)]
    async fn sleeper_inputs(
        &self,
        season: u32,
        force: bool,
    ) -> Result<
        (
            (
                u64,
                HashMap<String, crate::sleeper::PlayerMeta>,
                Option<String>,
            ),
            (u64, Vec<crate::sleeper::ProjectionRow>, Option<String>),
            (u64, Vec<crate::sleeper::ProjectionRow>, Option<String>),
        ),
        String,
    > {
        tokio::try_join!(
            self.players(force),
            self.season_projections(season, force),
            self.weekly_projections(season, force),
        )
    }
}

/// The crosswalk keyed the way a pick names a player: `yahoo:<id>` -> board id.
fn crossed_ids(crosswalk: &Crosswalk) -> HashMap<String, String> {
    crosswalk
        .ids
        .iter()
        .map(|(key, id)| (yahoo_map::player_id(key), id.clone()))
        .collect()
}

/// The team whose manager is the logged-in user, if this account manages one.
pub(crate) fn current_login(teams: &[YahooTeam]) -> Option<&YahooTeam> {
    teams
        .iter()
        .find(|team| team.managers.iter().any(|m| m.is_current_login))
}

/// Team key -> team name, which is what the board labels a draft slot with.
pub(crate) fn team_names(teams: &[YahooTeam]) -> HashMap<String, String> {
    teams
        .iter()
        .map(|team| (team.team_key.clone(), team.name.clone()))
        .collect()
}

/// Yahoo draft results as the app's picks, with the player ids crossed over
/// to Sleeper's. Shared with the poller, which fetches the same two resources.
pub fn picks_for(
    results: &[YahooDraftPick],
    teams: &[YahooTeam],
    pool: &[YahooPlayer],
    crosswalk: &Crosswalk,
) -> Vec<Pick> {
    let by_key: HashMap<String, YahooPlayer> = pool
        .iter()
        .map(|player| (player.player_key.clone(), player.clone()))
        .collect();
    // `yahoo_map::picks` names a player `yahoo:<id>`; a player the crosswalk
    // matched sits on the board under his Sleeper id instead, and a pick that
    // named the other one would never take him off it.
    let crossed: HashMap<String, String> = results
        .iter()
        .filter(|result| !result.player_key.is_empty())
        .filter_map(|result| {
            let id = crosswalk.id_for(&result.player_key)?;
            Some((yahoo_map::player_id(&result.player_key), id.to_string()))
        })
        .collect();
    let mut picks = yahoo_map::picks(results, teams, &by_key);
    for pick in &mut picks {
        if let Some(id) = crossed.get(&pick.player_id) {
            pick.player_id = id.clone();
        }
    }
    picks
}

/// A synthesized `Draft` for a Yahoo league.
///
/// Yahoo has no draft resource: the draft is addressed by the league key and
/// its shape is spread across the league settings and the team list. Every
/// field that has no Yahoo counterpart is defaulted and said so here.
pub fn draft_for(league_key: &str, yahoo: &YahooLeague, teams: &[YahooTeam]) -> Draft {
    let rounds = rounds_from(&yahoo.roster_positions);
    Draft {
        // The league key doubles as the draft id, which is what the manual
        // pick and keeper files are named after.
        draft_id: league_key.to_string(),
        status: draft_status(&yahoo.draft_status),
        // Yahoo answers `draft_type: "live"` for a live auction and flags the
        // auction separately, so both have to be read: on the type alone every
        // auction league would be drawn as a snake board.
        draft_type: if yahoo.is_auction_draft || yahoo.draft_type.as_deref() == Some("auction") {
            "auction".to_string()
        } else {
            "snake".to_string()
        },
        settings: DraftSettings {
            teams: if yahoo.num_teams > 0 {
                yahoo.num_teams
            } else {
                teams.len() as u32
            },
            rounds,
            // Yahoo's `draft_pick_time` is in the settings payload but not on
            // the shape this app parses, so there is no clock to count down.
            pick_timer: None,
            // Third-round reversal is a Sleeper option; Yahoo has none.
            reversal_round: None,
            // The roster shape is read off the league, which a Yahoo league
            // always has — these exist for leagueless Sleeper mock drafts.
            slots_qb: None,
            slots_rb: None,
            slots_wr: None,
            slots_te: None,
            slots_flex: None,
            slots_super_flex: None,
            slots_k: None,
            slots_def: None,
        },
        draft_order: Some(
            teams
                .iter()
                .filter_map(|team| Some((team.team_key.clone(), team.draft_position?)))
                .collect(),
        ),
        start_time: yahoo.draft_time.map(|at| (at as i64) * 1000),
        season: Some(yahoo.season.clone()),
        metadata: None,
        // "Who created it" has no Yahoo equivalent, and "my team" is answered
        // by `LoadedLeague::my_slot` rather than guessed at from this.
        creators: None,
        // Yahoo does not timestamp the last pick, so the draft screen shows no
        // pick clock for a Yahoo league.
        last_picked: None,
        // Only traded picks need the slot-to-roster bridge, and Yahoo trades
        // none through this API.
        slot_to_roster_id: None,
    }
}

/// Yahoo's `draft_status` as a Sleeper draft status.
fn draft_status(yahoo: &str) -> String {
    match yahoo.trim().to_ascii_lowercase().as_str() {
        "draft" | "drafting" => "drafting",
        "postdraft" => "complete",
        _ => "pre_draft",
    }
    .to_string()
}

/// How many rounds a draft of this roster runs to: one per seat, bar the
/// injured-reserve slots, which are never drafted into.
fn rounds_from(slots: &[crate::yahoo_types::RosterSlot]) -> u32 {
    slots
        .iter()
        .filter(|slot| {
            let name = yahoo_map::roster_position(&slot.position);
            name != "IR" && name != "IL"
        })
        .map(|slot| slot.count)
        .sum()
}

#[cfg(test)]
#[path = "engine_yahoo_tests.rs"]
mod tests;
