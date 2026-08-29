//! The season so far: the record and standings Sleeper keeps, each past
//! week's result for the user, and how every player on the user's roster has
//! scored against what he was projected — the honest sell-high / buy-low
//! signal once a few weeks are in.
//!
//! Actuals come from the past weeks' `/matchups`, which carry each player's
//! points under the league's own scoring; nothing here re-scores a box score.

use crate::draft::TeamRoster;
use crate::loaded::LoadedLeague;
use crate::sleeper::{LeagueRoster, Matchup};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct StandingRow {
    pub slot: u32,
    pub display_name: Option<String>,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub points_for: f64,
    pub points_against: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekResult {
    pub week: u32,
    pub my_points: f64,
    pub opponent_slot: Option<u32>,
    pub opponent_name: Option<String>,
    pub opponent_points: Option<f64>,
    /// `None` for a week with no opponent (bye week in the schedule).
    pub won: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerTrend {
    pub player_id: String,
    pub name: String,
    pub position: String,
    /// Weeks with both a projection and a score.
    pub games: u32,
    pub projected: f64,
    pub actual: f64,
    /// actual − projected, per game.
    pub delta_per_game: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeasonSoFar {
    /// Through this week.
    pub through_week: u32,
    pub standings: Vec<StandingRow>,
    pub my_results: Vec<WeekResult>,
    /// My roster, most over-performing first.
    pub trends: Vec<PlayerTrend>,
}

/// Roster id -> slot, from the draft's map.
pub type SlotOf<'a> = &'a dyn Fn(u32) -> Option<u32>;

pub fn standings(
    rosters: &[LeagueRoster],
    slot_of: SlotOf,
    name_of_slot: &dyn Fn(u32) -> Option<String>,
) -> Vec<StandingRow> {
    let mut rows: Vec<StandingRow> = rosters
        .iter()
        .filter_map(|r| {
            let slot = slot_of(r.roster_id)?;
            let s = &r.settings;
            Some(StandingRow {
                slot,
                display_name: name_of_slot(slot),
                wins: s.wins,
                losses: s.losses,
                ties: s.ties,
                points_for: f64::from(s.fpts) + f64::from(s.fpts_decimal) / 100.0,
                points_against: f64::from(s.fpts_against)
                    + f64::from(s.fpts_against_decimal) / 100.0,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.wins
            .cmp(&a.wins)
            .then(a.losses.cmp(&b.losses))
            .then(b.points_for.total_cmp(&a.points_for))
    });
    rows
}

pub fn my_results(
    past: &[(u32, Vec<Matchup>)],
    my_roster_id: u32,
    slot_of: SlotOf,
    name_of_slot: &dyn Fn(u32) -> Option<String>,
) -> Vec<WeekResult> {
    past.iter()
        .filter_map(|(week, matchups)| {
            let mine = matchups.iter().find(|m| m.roster_id == my_roster_id)?;
            let opp = mine.matchup_id.and_then(|id| {
                matchups
                    .iter()
                    .find(|m| m.matchup_id == Some(id) && m.roster_id != my_roster_id)
            });
            let opponent_slot = opp.and_then(|o| slot_of(o.roster_id));
            Some(WeekResult {
                week: *week,
                my_points: mine.points,
                opponent_slot,
                opponent_name: opponent_slot.and_then(name_of_slot),
                opponent_points: opp.map(|o| o.points),
                won: opp.map(|o| mine.points > o.points),
            })
        })
        .collect()
}

/// Projected vs actual for the players on `roster_ids` (my current roster),
/// over every past week where both numbers exist.
pub fn trends(
    past: &[(u32, Vec<Matchup>)],
    players: &[(String, String, String)],
    weekly_points: &HashMap<String, Vec<(u32, f64)>>,
) -> Vec<PlayerTrend> {
    // player -> week -> actual, from any roster's matchup row (a player
    // traded mid-season scored for someone else, and that still counts).
    let mut actual: HashMap<&str, HashMap<u32, f64>> = HashMap::new();
    for (week, matchups) in past {
        for m in matchups {
            for (pid, pts) in &m.players_points {
                actual.entry(pid.as_str()).or_default().insert(*week, *pts);
            }
        }
    }
    let mut out: Vec<PlayerTrend> = players
        .iter()
        .map(|(id, name, position)| {
            let scored = actual.get(id.as_str());
            let projected_by_week = weekly_points.get(id);
            let mut games = 0;
            let mut projected = 0.0;
            let mut actual_sum = 0.0;
            for (week, _) in past {
                let (Some(a), Some(p)) = (
                    scored.and_then(|s| s.get(week)),
                    projected_by_week.and_then(|w| w.iter().find(|(wk, _)| wk == week)),
                ) else {
                    continue;
                };
                games += 1;
                projected += p.1;
                actual_sum += a;
            }
            PlayerTrend {
                player_id: id.clone(),
                name: name.clone(),
                position: position.clone(),
                games,
                projected,
                actual: actual_sum,
                delta_per_game: if games > 0 {
                    (actual_sum - projected) / f64::from(games)
                } else {
                    0.0
                },
            }
        })
        .filter(|t| t.games > 0)
        .collect();
    out.sort_by(|a, b| b.delta_per_game.total_cmp(&a.delta_per_game));
    out
}

/// The season so far for `my_slot`, or nothing before a week has been played.
pub fn season_so_far(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    my_slot: Option<u32>,
) -> Option<SeasonSoFar> {
    if loaded.past_matchups.is_empty() {
        return None;
    }
    let slot_to_roster: HashMap<u32, u32> = loaded
        .draft
        .slot_to_roster_id
        .as_ref()?
        .iter()
        .filter_map(|(s, r)| s.parse().ok().map(|s: u32| (s, *r)))
        .collect();
    let slot_of = |roster_id: u32| {
        slot_to_roster
            .iter()
            .find(|(_, r)| **r == roster_id)
            .map(|(s, _)| *s)
    };
    let name_of_slot = |slot: u32| {
        rosters
            .get((slot - 1) as usize)
            .and_then(|r| r.display_name.clone())
    };
    let through_week = loaded.past_matchups.iter().map(|(w, _)| *w).max()?;
    let standings = standings(&loaded.league_rosters, &slot_of, &name_of_slot);
    let mine =
        my_slot.and_then(|s| Some((s, *slot_to_roster.get(&s)?, rosters.get((s - 1) as usize)?)));
    let (my_results, trends) = match mine {
        Some((_, my_roster_id, roster)) => {
            let players: Vec<(String, String, String)> = roster
                .players
                .iter()
                .map(|p| (p.player_id.clone(), p.name.clone(), p.position.clone()))
                .collect();
            (
                my_results(&loaded.past_matchups, my_roster_id, &slot_of, &name_of_slot),
                trends(&loaded.past_matchups, &players, &loaded.weekly_points),
            )
        }
        None => (Vec::new(), Vec::new()),
    };
    Some(SeasonSoFar {
        through_week,
        standings,
        my_results,
        trends,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sleeper::RosterSettings;

    fn m(roster_id: u32, matchup_id: u32, points: f64, pp: &[(&str, f64)]) -> Matchup {
        Matchup {
            roster_id,
            matchup_id: Some(matchup_id),
            starters: Vec::new(),
            players: Vec::new(),
            points,
            players_points: pp.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    fn slot_of(roster_id: u32) -> Option<u32> {
        Some(roster_id + 10)
    }
    fn name_of(slot: u32) -> Option<String> {
        Some(format!("Team{slot}"))
    }

    #[test]
    fn results_name_the_opponent_and_say_who_won() {
        let past = vec![
            (1, vec![m(1, 7, 120.0, &[]), m(2, 7, 110.5, &[])]),
            (2, vec![m(1, 3, 90.0, &[]), m(2, 3, 95.0, &[])]),
            (3, vec![m(1, 1, 100.0, &[])]),
        ];
        let r = my_results(&past, 1, &slot_of, &name_of);
        assert_eq!(r.len(), 3);
        assert_eq!(
            (r[0].week, r[0].won, r[0].opponent_name.as_deref()),
            (1, Some(true), Some("Team12"))
        );
        assert_eq!((r[1].won, r[1].opponent_points), (Some(false), Some(95.0)));
        assert_eq!(r[2].won, None, "no opponent, no result");
    }

    #[test]
    fn standings_sort_by_record_then_points() {
        let row = |id: u32, w: u32, l: u32, fpts: u32, dec: u32| LeagueRoster {
            roster_id: id,
            owner_id: None,
            settings: RosterSettings {
                wins: w,
                losses: l,
                ties: 0,
                fpts,
                fpts_decimal: dec,
                fpts_against: 0,
                fpts_against_decimal: 0,
                waiver_budget_used: 0,
                waiver_position: None,
            },
            starters: Vec::new(),
            players: Vec::new(),
        };
        let s = standings(
            &[
                row(1, 2, 1, 300, 50),
                row(2, 3, 0, 250, 0),
                row(3, 2, 1, 310, 0),
            ],
            &slot_of,
            &name_of,
        );
        let order: Vec<u32> = s.iter().map(|r| r.slot).collect();
        assert_eq!(order, vec![12, 13, 11]);
        assert_eq!(s[2].points_for, 300.5);
    }

    #[test]
    fn trends_compare_actual_to_the_projection_for_the_same_week_only() {
        let past = vec![
            (1, vec![m(1, 7, 0.0, &[("a", 20.0), ("b", 5.0)])]),
            (2, vec![m(1, 7, 0.0, &[("a", 10.0)])]),
        ];
        let weekly = HashMap::from([
            ("a".to_string(), vec![(1, 12.0), (2, 12.0), (3, 12.0)]),
            ("b".to_string(), vec![(2, 8.0)]), // no projection for week 1
        ]);
        let players = vec![
            ("a".to_string(), "A".to_string(), "WR".to_string()),
            ("b".to_string(), "B".to_string(), "RB".to_string()),
            ("c".to_string(), "C".to_string(), "TE".to_string()),
        ];
        let t = trends(&past, &players, &weekly);
        assert_eq!(
            t.len(),
            1,
            "b has no week with both numbers, c has nothing: {t:?}"
        );
        assert_eq!((t[0].games, t[0].projected, t[0].actual), (2, 24.0, 30.0));
        assert!((t[0].delta_per_game - 3.0).abs() < 1e-9);
    }
}
