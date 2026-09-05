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
use std::collections::HashMap;
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

impl PlayerRefreshData {
    /// True when this is worth applying. An empty dictionary is what a failed
    /// parse looks like, and swapping it in would blank every name on screen.
    pub fn is_usable(&self) -> bool {
        !self.players.is_empty()
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
fn restamp_injuries(board: &mut [BoardPlayer], players: &HashMap<String, PlayerMeta>) {
    for player in board.iter_mut() {
        player.injury_status = players
            .get(&player.player_id)
            .and_then(|meta| meta.injury_status.clone())
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

    #[test]
    fn an_empty_dictionary_is_never_swapped_in() {
        assert!(!refresh_from(HashMap::new(), 1, Vec::new(), 1).is_usable());
        assert!(refresh_from(
            HashMap::from([("rb1".to_string(), meta(None))]),
            1,
            Vec::new(),
            1
        )
        .is_usable());
    }
}
