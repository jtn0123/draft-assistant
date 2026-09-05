//! Keeping the player dictionary and the weekly projections current while the
//! app is left open.
//!
//! Both are fetched once, by the user-driven league load, and every reader
//! afterwards — the season poller most of all — takes them off the
//! `LoadedLeague` that load produced. That is fine for an app opened and
//! closed inside an afternoon and wrong for one left running: a Sunday-morning
//! downgrade to Out, or Wednesday's re-projection of the coming week, never
//! reached the screen until somebody reloaded the league by hand. The Games
//! tab, the start/sit calls and the playoff odds all read those two, so they
//! were quietly frozen for the whole life of the process.

use crate::board::BoardPlayer;
use crate::engine::{Engine, LoadedLeague};
use crate::sleeper::{PlayerMeta, ProjectionRow};
use crate::weekly::WeeklyPoints;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A refreshed dictionary and projection set, ready to be swapped in.
///
/// Fetched with nothing locked and applied afterwards, because the dictionary
/// alone is ~14.6 MB over the wire: holding the loaded league across that
/// would stall every command and both pollers for the length of it.
pub struct PlayerRefreshData {
    players: HashMap<String, PlayerMeta>,
    players_at: u64,
    weekly: Vec<ProjectionRow>,
    weekly_at: u64,
}

/// How much of the roster-and-board population a refreshed dictionary has to
/// know before it is allowed to replace the loaded one.
///
/// Sleeper's dictionary answers with every player it has, so a response that
/// knows only a fraction of the people actually in the league is a truncated
/// or half-parsed one, not news. Eighty per cent leaves room for the handful
/// of ids that legitimately go missing when a player is cut from the league
/// entirely, and still refuses a body that lost most of itself in transit.
const COVERAGE: f64 = 0.8;

/// The players a refresh has to know about: everyone on a roster, plus
/// everyone on the draft board the screen still renders names for.
///
/// Borrowed rather than owned so building one costs no allocation per id; the
/// caller holds the league and the season while it checks.
pub fn wanted_ids<'a>(
    board_ids: impl Iterator<Item = &'a str>,
    roster_ids: impl Iterator<Item = &'a str>,
) -> HashSet<&'a str> {
    board_ids.chain(roster_ids).collect()
}

impl PlayerRefreshData {
    /// True when this is worth applying.
    ///
    /// An empty dictionary is what a failed parse looks like, and swapping it
    /// in would blank every name on screen. A *partial* one is the subtler
    /// version of the same failure and used to pass this check: a truncated
    /// body still deserialises, and applying it dropped every player it had
    /// lost — names, injury tags and all — from the loaded league. So the
    /// refresh also has to still know most of the people in this league.
    pub fn is_usable(&self, wanted: &HashSet<&str>) -> bool {
        if self.players.is_empty() {
            return false;
        }
        if wanted.is_empty() {
            return true;
        }
        let known = wanted
            .iter()
            .filter(|id| self.players.contains_key(**id))
            .count();
        known as f64 >= COVERAGE * wanted.len() as f64
    }

    /// Swap the refreshed halves into a loaded league.
    ///
    /// An empty projection set is left alone rather than rebuilt from. It is
    /// what a fetch that answered with nothing looks like, and rebuilding from
    /// it would zero every projection on the screen — no start/sit calls, no
    /// waiver targets, an empty matchup table — which is far worse than
    /// carrying yesterday's numbers for another half hour.
    pub fn apply(self, loaded: &mut LoadedLeague) {
        restamp_injuries(Arc::<Vec<_>>::make_mut(&mut loaded.board), &self.players);
        if !self.weekly.is_empty() {
            loaded.weekly_points = Arc::new(WeeklyPoints::build(
                &self.weekly,
                &loaded.league.scoring_settings,
            ));
            loaded.weekly_fetched_at = self.weekly_at;
        }
        loaded.player_meta = Arc::new(self.players);
        loaded.players_fetched_at = self.players_at;
    }
}

/// Rewrite every board row's injury status from a freshly fetched dictionary.
///
/// [`crate::season_lookup::Lookup`] reads the board first and only falls back
/// to the dictionary, so swapping the dictionary alone would leave every
/// drafted player — which is to say everyone who matters — stamped with the
/// status he carried when the league was opened.
///
/// A player the dictionary says nothing about is left exactly as he was. The
/// dictionary can be short of a few ids even when it is healthy enough to
/// apply, and reading absence as "no longer injured" would quietly clear an
/// Out tag and put the player back in the optimal lineup.
fn restamp_injuries(board: &mut [BoardPlayer], players: &HashMap<String, PlayerMeta>) {
    for player in board.iter_mut() {
        let Some(meta) = players.get(&player.player_id) else {
            continue;
        };
        player.injury_status = meta
            .injury_status
            .clone()
            .filter(|status| !status.trim().is_empty());
    }
}

/// Re-fetching the slow-moving player data behind a season view.
///
/// A trait so the season poller can be driven by a stub in the tests, the same
/// way [`crate::season_engine::SeasonLoader`] is.
pub trait PlayerRefresh {
    /// Fetch a fresh dictionary and weekly projection set for `season`, or
    /// `None` when nothing usable came back. A failure is not reported: the
    /// data already loaded is still the best answer available, and the live
    /// feed's own health badge is what tells the user the network is down.
    #[allow(async_fn_in_trait)]
    async fn refresh_players(&self, season: u32) -> Option<PlayerRefreshData>;
}

impl PlayerRefresh for Engine {
    async fn refresh_players(&self, season: u32) -> Option<PlayerRefreshData> {
        // `force` is off: both fetchers serve a fresh cache without a request,
        // so a poller asking every half hour costs nothing until the cached
        // copy ages past its own TTL.
        let (players, weekly) =
            tokio::join!(self.players(false), self.weekly_projections(season, false));
        let (players_at, players, _) = players.ok()?;
        let (weekly_at, weekly, _) = weekly.ok()?;
        Some(PlayerRefreshData {
            players,
            players_at,
            weekly,
            weekly_at,
        })
    }
}

/// Build a refresh out of already-fetched parts. The poller's tests hand these
/// in directly rather than standing a dictionary endpoint up.
pub fn refresh_from(
    players: HashMap<String, PlayerMeta>,
    players_at: u64,
    weekly: Vec<ProjectionRow>,
    weekly_at: u64,
) -> PlayerRefreshData {
    PlayerRefreshData {
        players,
        players_at,
        weekly,
        weekly_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(injury: Option<&str>) -> PlayerMeta {
        serde_json::from_value(serde_json::json!({ "injury_status": injury })).unwrap()
    }

    fn board_player(id: &str, injury: Option<&str>) -> BoardPlayer {
        BoardPlayer {
            player_id: id.to_string(),
            name: "A Player".to_string(),
            position: "RB".to_string(),
            team: Some("ATL".to_string()),
            bye_week: None,
            points: 100.0,
            bonus_points: 0.0,
            vorp: 10.0,
            tier: 1,
            position_rank: 1,
            overall_rank: 1,
            adp: None,
            injury_status: injury.map(str::to_string),
            sleeper_pts_ppr: None,
            second_opinion: None,
            weekly_cv: None,
        }
    }

    /// The bug: the dictionary was swapped and the board was not, so
    /// `Lookup::injury` — which reads the board first — kept handing back the
    /// status the player carried when the league was opened.
    #[test]
    fn a_refresh_rewrites_the_board_status_as_well_as_the_dictionary() {
        let mut board = vec![board_player("rb1", None), board_player("rb2", Some("Q"))];
        let players = HashMap::from([
            ("rb1".to_string(), meta(Some("Out"))),
            ("rb2".to_string(), meta(Some("   "))),
        ]);
        restamp_injuries(&mut board, &players);

        assert_eq!(board[0].injury_status.as_deref(), Some("Out"));
        assert_eq!(
            board[1].injury_status, None,
            "a status cleared upstream must clear here, not linger from the load"
        );
    }

    /// The bug: a dictionary short of most of the league still passed the
    /// "not empty" check, and applying it dropped every player it had lost.
    #[test]
    fn a_dictionary_missing_most_of_the_league_is_never_swapped_in() {
        let full: HashMap<String, PlayerMeta> =
            (0..10).map(|i| (format!("p{i}"), meta(None))).collect();
        let ids: Vec<String> = (0..10).map(|i| format!("p{i}")).collect();
        let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();

        assert!(!refresh_from(HashMap::new(), 1, Vec::new(), 1).is_usable(&wanted));
        assert!(refresh_from(full.clone(), 1, Vec::new(), 1).is_usable(&wanted));

        let half: HashMap<String, PlayerMeta> = full
            .iter()
            .take(5)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert!(
            !refresh_from(half, 1, Vec::new(), 1).is_usable(&wanted),
            "a dictionary knowing half the league was swapped in"
        );

        let most: HashMap<String, PlayerMeta> = full
            .iter()
            .take(8)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert!(
            refresh_from(most, 1, Vec::new(), 1).is_usable(&wanted),
            "a dictionary missing two of ten is normal turnover, not a failure"
        );
    }

    /// The bug: a player the refreshed dictionary had never heard of had his
    /// injury tag cleared, which put a player listed Out back in the lineup.
    #[test]
    fn a_player_missing_from_the_refresh_keeps_the_status_he_had() {
        let mut board = vec![board_player("rb1", Some("Out")), board_player("rb2", None)];
        let players = HashMap::from([("rb2".to_string(), meta(Some("Q")))]);
        restamp_injuries(&mut board, &players);
        assert_eq!(board[0].injury_status.as_deref(), Some("Out"));
        assert_eq!(board[1].injury_status.as_deref(), Some("Q"));
    }
}
