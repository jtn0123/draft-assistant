//! Team strength over time, and why it moved.
//!
//! Every Season load takes a snapshot of each roster's projected strength —
//! the mean points of its best possible lineup per remaining week — and the
//! per-player projections behind it. Snapshots are kept on disk per league,
//! at most one per six hours unless a roster actually changed hands, so the
//! series moves when the league moves rather than on every refresh.
//!
//! Diffing consecutive snapshots gives the "why": a player who appears or
//! disappears is matched to the transaction that moved them (trade, waiver
//! claim, free-agent add, drop), and a player whose projection shifted is
//! reported with the shift and, if it changed, their injury tag.

use crate::cache::safe_key;
use crate::engine::{now_secs, Engine, LoadedLeague};
use crate::season_engine::LoadedSeason;
use crate::season_lineup::weekly_lineup_totals;
use crate::season_lookup::Lookup;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Minimum spacing between routine snapshots.
pub const MIN_GAP_SECS: u64 = 6 * 3600;
/// Roughly a season of six-hourly snapshots plus roster-change extras.
pub const MAX_SNAPSHOTS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnap {
    /// Mean projected points per remaining regular-season week.
    pub points: f64,
    #[serde(default)]
    pub injury: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSnap {
    pub roster_id: u32,
    /// Mean best-lineup points per remaining week.
    pub strength: f64,
    pub players: HashMap<String, PlayerSnap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Seconds since epoch.
    pub taken_at: u64,
    pub week: u32,
    pub teams: Vec<TeamSnap>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub snapshots: Vec<Snapshot>,
}

/// Measure every roster right now.
///
/// Positions come from the shared `Lookup`, not from `player_meta` directly:
/// the board carries the position this league actually scores a player at, and
/// the standings projection resolves through the same `Lookup`. Reading the
/// metadata here instead meant a team's Trends strength and its standings
/// projection could be built from two different lineups for the same roster.
pub fn take_snapshot(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    lookup: &Lookup,
    now: u64,
) -> Snapshot {
    let weekly = &loaded.weekly_points;
    let rules = &loaded.roster_rules;
    let first = season.week.max(1);
    let last = loaded.league.last_regular_week().max(first);
    let weeks = f64::from(last - first + 1);
    let position_of = |id: &str| lookup.position(id);

    let teams = season
        .rosters
        .iter()
        .map(|roster| {
            let ids = roster.player_ids();
            let total: f64 = weekly_lineup_totals(rules, ids, &position_of, weekly, first..=last)
                .iter()
                .map(|(_, points)| points)
                .sum();
            let players = ids
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        PlayerSnap {
                            points: weekly.mean_from(id, first, last),
                            injury: loaded
                                .player_meta
                                .get(id)
                                .and_then(|m| m.injury_status.clone()),
                        },
                    )
                })
                .collect();
            TeamSnap {
                roster_id: roster.roster_id,
                strength: total / weeks,
                players,
            }
        })
        .collect();

    Snapshot {
        taken_at: now,
        week: season.week,
        teams,
    }
}

fn roster_sets(snapshot: &Snapshot) -> HashMap<u32, HashSet<&str>> {
    snapshot
        .teams
        .iter()
        .map(|t| (t.roster_id, t.players.keys().map(String::as_str).collect()))
        .collect()
}

/// Record when enough time has passed, or when any roster changed — a trade
/// should show up on the graph the next time the screen opens, not six hours
/// later.
pub fn should_record(history: &History, next: &Snapshot) -> bool {
    let Some(last) = history.snapshots.last() else {
        return true;
    };
    if next.taken_at.saturating_sub(last.taken_at) >= MIN_GAP_SECS {
        return true;
    }
    roster_sets(last) != roster_sets(next)
}

pub fn push(history: &mut History, snapshot: Snapshot) {
    history.snapshots.push(snapshot);
    if history.snapshots.len() > MAX_SNAPSHOTS {
        let excess = history.snapshots.len() - MAX_SNAPSHOTS;
        history.snapshots.drain(..excess);
    }
}

/// Persisting the Trends snapshots for a league.
pub trait HistoryStore {
    /// Load this league's history, add a snapshot if one is due, and persist.
    fn record_history(&self, loaded: &LoadedLeague, season: &LoadedSeason) -> History;
}

impl Engine {
    fn history_name(league_id: &str) -> String {
        // Sanitized like every other cache filename: the id comes back from
        // Sleeper's own response today, but this must not be the one place a
        // traversal could land.
        format!("history_{}.json", safe_key(league_id))
    }
}

impl HistoryStore for Engine {
    fn record_history(&self, loaded: &LoadedLeague, season: &LoadedSeason) -> History {
        let name = Self::history_name(&loaded.league.league_id);
        let mut history: History = self
            .read_cache_any(&name)
            .map(|(_, h)| h)
            .unwrap_or_default();
        let lookup = Lookup { loaded };
        let snapshot = take_snapshot(loaded, season, &lookup, now_secs());
        if should_record(&history, &snapshot) {
            push(&mut history, snapshot);
            self.write_cache(&name, &history);
        }
        history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(roster_id: u32, strength: f64, players: &[(&str, f64)]) -> TeamSnap {
        TeamSnap {
            roster_id,
            strength,
            players: players
                .iter()
                .map(|(id, points)| {
                    (
                        (*id).to_string(),
                        PlayerSnap {
                            points: *points,
                            injury: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn snapshot(at: u64, teams: Vec<TeamSnap>) -> Snapshot {
        Snapshot {
            taken_at: at,
            week: 3,
            teams,
        }
    }

    #[test]
    fn a_roster_change_forces_a_snapshot_inside_the_quiet_window() {
        let mut history = History::default();
        let first = snapshot(1_000, vec![team(1, 100.0, &[("a", 10.0)])]);
        assert!(should_record(&history, &first));
        push(&mut history, first);
        let same = snapshot(2_000, vec![team(1, 101.0, &[("a", 11.0)])]);
        assert!(
            !should_record(&history, &same),
            "projection drift alone is not news"
        );
        let moved = snapshot(2_000, vec![team(1, 101.0, &[("b", 11.0)])]);
        assert!(should_record(&history, &moved));
        let later = snapshot(1_000 + MIN_GAP_SECS, vec![team(1, 101.0, &[("a", 11.0)])]);
        assert!(should_record(&history, &later));
    }

    #[test]
    fn history_is_capped() {
        let mut history = History::default();
        for i in 0..(MAX_SNAPSHOTS as u64 + 5) {
            push(&mut history, snapshot(i, vec![]));
        }
        assert_eq!(history.snapshots.len(), MAX_SNAPSHOTS);
        assert_eq!(history.snapshots[0].taken_at, 5);
    }
}
