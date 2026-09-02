//! Roll-ups over the joined scoreboard: the head-to-head totals and the
//! kickoff windows the games are shown in.
//!
//! Separate from the join in `season_live` because nothing here looks at
//! Sleeper at all — these read only the `LiveGame`s the join produced, which
//! is what makes them the cheap part to recompute on every poll tick.

use crate::season_live::{GameState, LiveGame, PlayState};
use serde::Serialize;
use std::collections::HashMap;

/// Running totals across every game, for the head-to-head bar and the
/// playing / yet-to-play / done counters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LiveTotals {
    pub my_playing: usize,
    pub my_pre: usize,
    pub my_done: usize,
    /// Points already banked (players whose game has started).
    pub my_live_points: f64,
    pub opp_live_points: f64,
}

/// Totals across every tracked player, driven by their game's state.
pub fn totals(games: &[LiveGame]) -> LiveTotals {
    let mut totals = LiveTotals::default();
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for game in games {
        for chip in &game.chips {
            // A player appears in exactly one game, but guard anyway: a stale
            // team field must never double-count somebody's points.
            if seen.insert(chip.player_id.as_str(), ()).is_some() {
                continue;
            }
            if chip.is_mine {
                match chip.state {
                    PlayState::Playing => totals.my_playing += 1,
                    PlayState::Pre => totals.my_pre += 1,
                    PlayState::Done => totals.my_done += 1,
                }
            }
            if chip.state == PlayState::Pre {
                continue;
            }
            if chip.is_mine {
                totals.my_live_points += chip.points;
            } else {
                totals.opp_live_points += chip.points;
            }
        }
    }
    totals
}

/// Games grouped into kickoff windows, in chronological order.
#[derive(Debug, Clone, Serialize)]
pub struct KickoffWindow {
    pub kickoff_ms: i64,
    pub my_starters: usize,
    pub games: Vec<LiveGame>,
}

pub fn windows(games: &[LiveGame]) -> Vec<KickoffWindow> {
    let mut order: Vec<i64> = Vec::new();
    let mut grouped: HashMap<i64, Vec<LiveGame>> = HashMap::new();
    for game in games {
        if !grouped.contains_key(&game.kickoff_ms) {
            order.push(game.kickoff_ms);
        }
        grouped
            .entry(game.kickoff_ms)
            .or_default()
            .push(game.clone());
    }
    order.sort_unstable();
    order
        .into_iter()
        .map(|kickoff_ms| {
            let games = grouped.remove(&kickoff_ms).unwrap_or_default();
            KickoffWindow {
                my_starters: games.iter().map(LiveGame::my_starter_count).sum(),
                kickoff_ms,
                games,
            }
        })
        .collect()
}

/// The next window that has not kicked off yet.
pub fn next_window(windows: &[KickoffWindow]) -> Option<&KickoffWindow> {
    windows
        .iter()
        .find(|w| w.games.iter().all(|g| g.state == GameState::Pre))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::season_live::fixtures::*;
    use crate::season_live::live_games;

    #[test]
    fn totals_count_only_started_games_and_split_by_side() {
        let games = vec![
            game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", "")),
            game("b", "ATL", "CAR", 2000, meta_final()),
            game("c", "PHI", "DAL", 3000, meta_pre()),
        ];
        let tracked = vec![
            player("mine-live", "PIT", 10.0, true),
            player("theirs-live", "BAL", 4.0, false),
            player("mine-done", "ATL", 21.0, true),
            player("mine-pre", "PHI", 22.0, true),
        ];
        let t = totals(&live_games(&games, &tracked));
        assert_eq!((t.my_playing, t.my_done, t.my_pre), (1, 1, 1));
        assert!((t.my_live_points - 31.0).abs() < 1e-9);
        assert!((t.opp_live_points - 4.0).abs() < 1e-9);
    }

    #[test]
    fn windows_group_by_kickoff_and_next_window_skips_started_ones() {
        let games = vec![
            game("a", "PIT", "BAL", 1000, meta_live(3, "07:12", "")),
            game("b", "HOU", "JAX", 1000, meta_live(2, "01:44", "")),
            game("c", "PHI", "DAL", 5000, meta_pre()),
        ];
        let tracked = vec![
            player("p1", "PIT", 9.0, true),
            player("p2", "HOU", 8.0, true),
            player("p3", "PHI", 22.0, true),
        ];
        let w = windows(&live_games(&games, &tracked));
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].games.len(), 2);
        assert_eq!(w[0].my_starters, 2);
        assert_eq!(next_window(&w).map(|w| w.kickoff_ms), Some(5000));
    }
}
