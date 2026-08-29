//! The waiver wire, priced for *this* roster: what each free agent would do
//! to the user's lineup, who else in the league he would help (the
//! competition for the claim), and how hard the wider Sleeper population is
//! adding him.
//!
//! "Would do to the lineup" is measured, not guessed: the bye-adjusted season
//! total with him on the roster minus without. A 90-point receiver behind
//! seven of the user's own is worth nothing here; a 60-point back who starts
//! on Bijan's bye is worth the games he starts.

use crate::draft::TeamRoster;
use crate::lineup::{self, Candidate};
use crate::loaded::LoadedLeague;
use crate::roster::RosterRules;
use crate::sleeper::TrendingPlayer;
use serde::Serialize;
use std::collections::HashMap;

/// Free agents considered, per position, by season points. Past this the
/// gain is zero for everyone.
const PER_POSITION: usize = 12;
/// How many targets and drop candidates to show.
const TARGETS: usize = 12;
const DROPS: usize = 4;
/// A rival "needs" a player if he lifts their season by at least this —
/// about a point a week. Below it nobody puts in a claim: a defense eight
/// points better than the one they have is not a move anyone makes.
const RIVAL_GAIN: f64 = 15.0;
/// At most this many targets per position, so twelve defenses cannot crowd
/// the one running back who would actually start out of the list.
const PER_POSITION_SHOWN: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct WaiverTarget {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub bye_week: Option<u32>,
    /// Season points under league scoring.
    pub points: f64,
    /// What he adds to my bye-adjusted season lineup total.
    pub my_gain: f64,
    /// Rivals whose lineup he would lift by `RIVAL_GAIN` or more: the
    /// number of other claims to expect.
    pub rivals_helped: u32,
    /// Sleeper-wide adds in the trending window, if he is on the list.
    pub trending_adds: Option<u64>,
    /// A FAAB bid in the league's own money, from what winning claims cost
    /// last season: the top targets at the 75th percentile, the middle at
    /// the median, the rest at half of it. Absent without history.
    pub suggested_bid: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DropCandidate {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub points: f64,
    /// Weeks he would start in the best lineup. Zero means never.
    pub starts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WaiverBoard {
    pub targets: Vec<WaiverTarget>,
    pub drops: Vec<DropCandidate>,
}

fn candidate(p: &crate::board::BoardPlayer) -> Candidate {
    Candidate {
        player_id: p.player_id.clone(),
        name: p.name.clone(),
        position: p.position.clone(),
        points: p.points,
        bye_week: p.bye_week,
        injury: p.injury_status.clone(),
    }
}

/// Season total with `extra` added to `roster`, minus without.
fn gain_for(roster: &[Candidate], base: f64, extra: &Candidate, rules: &RosterRules) -> f64 {
    let mut with = roster.to_vec();
    with.push(extra.clone());
    lineup::season_points(&with, rules) - base
}

/// Weeks each player on `roster` starts in the best lineup.
fn starts_per_player(roster: &[Candidate], rules: &RosterRules) -> HashMap<String, u32> {
    let mut starts: HashMap<String, u32> = HashMap::new();
    for week in 1..=17 {
        let active: Vec<Candidate> = roster
            .iter()
            .filter(|c| c.bye_week != Some(week))
            .cloned()
            .collect();
        for s in lineup::best_lineup(&active, rules).1 {
            *starts.entry(s.player_id).or_insert(0) += 1;
        }
    }
    starts
}

pub fn board(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    my_slot: u32,
    available: &[&crate::board::BoardPlayer],
    trending: &[TrendingPlayer],
) -> Option<WaiverBoard> {
    let rules = &loaded.roster_rules;
    let mine = rosters.get((my_slot - 1) as usize)?;
    let my_season = lineup::season_candidates(mine, &loaded.board, &loaded.board_index);
    let my_base = lineup::season_points(&my_season, rules);
    let rivals: Vec<(Vec<Candidate>, f64)> = rosters
        .iter()
        .filter(|r| r.slot != my_slot)
        .map(|r| {
            let season = lineup::season_candidates(r, &loaded.board, &loaded.board_index);
            let base = lineup::season_points(&season, rules);
            (season, base)
        })
        .collect();
    let trending: HashMap<&str, u64> = trending
        .iter()
        .map(|t| (t.player_id.as_str(), t.count))
        .collect();

    // The top few at each position the league starts; the board is sorted
    // by value already, so the first N seen per position are the best.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut targets: Vec<WaiverTarget> = available
        .iter()
        .filter(|p| {
            let n = seen.entry(p.position.as_str()).or_insert(0);
            *n += 1;
            *n <= PER_POSITION
        })
        .map(|p| {
            let c = candidate(p);
            let my_gain = gain_for(&my_season, my_base, &c, rules);
            let rivals_helped = rivals
                .iter()
                .filter(|(season, base)| gain_for(season, *base, &c, rules) >= RIVAL_GAIN)
                .count() as u32;
            WaiverTarget {
                player_id: p.player_id.clone(),
                name: p.name.clone(),
                position: p.position.clone(),
                team: p.team.clone(),
                bye_week: p.bye_week,
                points: p.points,
                my_gain,
                rivals_helped,
                trending_adds: trending.get(p.player_id.as_str()).copied(),
                suggested_bid: None,
            }
        })
        .collect();
    targets.sort_by(|a, b| {
        b.my_gain
            .total_cmp(&a.my_gain)
            .then(b.points.total_cmp(&a.points))
    });
    let mut shown: HashMap<String, usize> = HashMap::new();
    targets.retain(|t| {
        let n = shown.entry(t.position.clone()).or_insert(0);
        *n += 1;
        *n <= PER_POSITION_SHOWN
    });
    targets.truncate(TARGETS);
    if let Some(bids) = loaded
        .history
        .as_ref()
        .map(|h| &h.bids)
        .filter(|b| b.count > 0)
    {
        let worth: Vec<usize> = targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.my_gain >= 1.0)
            .map(|(i, _)| i)
            .collect();
        let n = worth.len();
        for (rank, i) in worth.into_iter().enumerate() {
            let by_rank = if rank * 3 < n {
                bids.p75
            } else if rank * 3 < 2 * n {
                bids.median
            } else {
                (bids.median / 2).max(1)
            };
            // Nobody pays a receiver's price for a defense or a kicker: they
            // are replaceable every week, and the market knows it. A token
            // bid gets the claim in a league that lets them through.
            targets[i].suggested_bid = Some(match targets[i].position.as_str() {
                "DEF" | "K" => (bids.median / 5).clamp(1, 10),
                _ => by_rank,
            });
        }
    }

    let starts = starts_per_player(&my_season, rules);
    let mut drops: Vec<DropCandidate> = my_season
        .iter()
        .map(|c| DropCandidate {
            player_id: c.player_id.clone(),
            name: c.name.clone(),
            position: c.position.clone(),
            points: c.points,
            starts: starts.get(&c.player_id).copied().unwrap_or(0),
        })
        .collect();
    drops.sort_by(|a, b| a.starts.cmp(&b.starts).then(a.points.total_cmp(&b.points)));
    drops.truncate(DROPS);

    Some(WaiverBoard { targets, drops })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, pos: &str, pts: f64, bye: Option<u32>) -> Candidate {
        Candidate {
            player_id: id.into(),
            name: id.into(),
            position: pos.into(),
            points: pts,
            bye_week: bye,
            injury: None,
        }
    }

    fn rules() -> RosterRules {
        RosterRules::new(
            &["RB", "WR", "FLEX", "BN"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_player_behind_the_starters_adds_nothing_and_one_who_starts_adds_his_games() {
        // Two backs, one on a bye in week 6; three receivers.
        let roster = vec![
            c("rb1", "RB", 200.0, Some(6)),
            c("wr1", "WR", 190.0, None),
            c("wr2", "WR", 150.0, None),
            c("wr3", "WR", 140.0, None),
        ];
        let base = lineup::season_points(&roster, &rules());
        // A fourth receiver behind three: never starts.
        let wr4 = gain_for(&roster, base, &c("wr4", "WR", 120.0, None), &rules());
        assert!(wr4.abs() < 1e-9, "{wr4}");
        // A back: starts in the RB slot on rb1's bye, and beats wr3 for
        // FLEX every other week (85 > 140? no — 85/17 = 5.0 vs 140/17 = 8.2).
        // So only the bye week: 85/17.
        let rb2 = gain_for(&roster, base, &c("rb2", "RB", 85.0, None), &rules());
        assert!((rb2 - 85.0 / 17.0).abs() < 1e-9, "{rb2}");
        // A better back takes FLEX from wr2 in the sixteen weeks rb1 plays
        // (+20/17 each) and the RB slot on the bye (+170/17).
        let rb_good = gain_for(&roster, base, &c("rb3", "RB", 170.0, None), &rules());
        let expected = 16.0 * (170.0 - 150.0) / 17.0 + 170.0 / 17.0;
        assert!((rb_good - expected).abs() < 1e-9, "{rb_good} vs {expected}");
    }

    #[test]
    fn drop_candidates_are_the_players_who_never_start() {
        let roster = vec![
            c("rb1", "RB", 200.0, None),
            c("wr1", "WR", 190.0, None),
            c("wr2", "WR", 150.0, None),
            c("wr3", "WR", 90.0, None),
            c("wr4", "WR", 60.0, None),
        ];
        let starts = starts_per_player(&roster, &rules());
        assert_eq!(starts.get("wr3"), None);
        assert_eq!(starts.get("wr4"), None);
        assert_eq!(starts.get("wr2"), Some(&17));
    }
}
