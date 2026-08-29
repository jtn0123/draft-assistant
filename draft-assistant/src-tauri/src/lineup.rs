//! What each roster is worth as a *lineup*: the best starters it can field,
//! and the points that lineup projects to over the season.
//!
//! Two numbers, because they answer different questions. `full_strength` is
//! the best lineup's season total, every starter available every week — the
//! number the user sees on a crib sheet. `season` walks the schedule one week
//! at a time, takes each player out on his bye, and starts the best of who is
//! left, so a team built from one offence's bye week pays for it here. That
//! second number is the one to rank teams by.

use crate::board::BoardPlayer;
use crate::draft::TeamRoster;
use crate::roster::RosterRules;
use serde::Serialize;
use std::collections::HashMap;

/// Fantasy regular season, and the weeks a season projection is spread over.
const WEEKS: u32 = 17;

/// The week to plan for when the calendar is unknown: the opener.
pub const OPENING_WEEK: u32 = 1;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub player_id: String,
    pub name: String,
    pub position: String,
    /// Season points under league scoring.
    pub points: f64,
    pub bye_week: Option<u32>,
    /// Sleeper's injury tag, if any ("Out", "IR", "Questionable" …).
    pub injury: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Starter {
    pub slot: String,
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub points: f64,
    /// Carried through so a lineup can say *why* a starter scores nothing.
    pub injury: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamProjection {
    pub slot: u32,
    pub display_name: Option<String>,
    /// Best lineup, season total, nobody ever on a bye.
    pub full_strength: f64,
    /// Week-by-week best lineup with byes honoured, summed.
    pub season: f64,
    /// The full-strength lineup, in roster-slot order.
    pub starters: Vec<Starter>,
    /// Which week `week_points` is for.
    pub week: u32,
    /// Best lineup from that week's own projections (matchups, byes), and
    /// what it scores. Zero for a team whose players have no rows that week.
    pub week_points: f64,
    pub week_starters: Vec<Starter>,
}

/// The best lineup from `candidates` for `slots`, greedy: dedicated slots
/// take the best at their position first, then each flex takes the best of
/// what is left among what it may hold, narrowest flex first. Greedy is
/// exact for one flex type and a close approximation when a league mixes
/// them — the same order `RosterRules::open_starting_slots` uses.
pub fn best_lineup(candidates: &[Candidate], rules: &RosterRules) -> (f64, Vec<Starter>) {
    let mut pool: Vec<&Candidate> = candidates.iter().collect();
    pool.sort_by(|a, b| b.points.total_cmp(&a.points));
    let mut used = vec![false; pool.len()];
    let mut lineup: Vec<Starter> = Vec::new();

    let mut take = |slot: &str, used: &mut Vec<bool>| {
        let pick = pool
            .iter()
            .enumerate()
            .find(|(i, c)| !used[*i] && RosterRules::can_fill(slot, &c.position));
        if let Some((i, c)) = pick {
            used[i] = true;
            lineup.push(Starter {
                slot: slot.to_string(),
                player_id: c.player_id.clone(),
                name: c.name.clone(),
                position: c.position.clone(),
                points: c.points,
                injury: c.injury.clone(),
            });
        }
    };

    let slots = rules.slots();
    for slot in slots {
        if RosterRules::is_non_starting(slot) || RosterRules::flex_eligible(slot).is_some() {
            continue;
        }
        take(slot, &mut used);
    }
    let mut flex: Vec<&String> = slots
        .iter()
        .filter(|s| RosterRules::flex_eligible(s).is_some())
        .collect();
    flex.sort_by_key(|s| RosterRules::flex_eligible(s).map_or(0, <[&str]>::len));
    for slot in flex {
        take(slot, &mut used);
    }
    // Back into roster order so the lineup reads like the league's own.
    let order = |s: &str| slots.iter().position(|x| x == s).unwrap_or(usize::MAX);
    lineup.sort_by_key(|s| order(&s.slot));
    let total = lineup.iter().map(|s| s.points).sum();
    (total, lineup)
}

/// Season points with byes honoured: each week, every player not on his bye
/// is worth a seventeenth of his season projection, and the best lineup from
/// those is what the team scores.
pub fn season_points(candidates: &[Candidate], rules: &RosterRules) -> f64 {
    (1..=WEEKS)
        .map(|week| {
            let active: Vec<Candidate> = candidates
                .iter()
                .filter(|c| c.bye_week != Some(week))
                .map(|c| Candidate {
                    points: c.points / f64::from(WEEKS),
                    ..c.clone()
                })
                .collect();
            best_lineup(&active, rules).0
        })
        .sum()
}

/// `week_candidates` carry that one week's projected points instead of the
/// season's; a player with no row for the week (bye, injured, not projected)
/// is simply absent from the list.
pub fn project(
    slot: u32,
    display_name: Option<String>,
    candidates: &[Candidate],
    week: u32,
    week_candidates: &[Candidate],
    rules: &RosterRules,
) -> TeamProjection {
    let (full_strength, starters) = best_lineup(candidates, rules);
    let (week_points, week_starters) = best_lineup(week_candidates, rules);
    TeamProjection {
        slot,
        display_name,
        full_strength,
        season: season_points(candidates, rules),
        starters,
        week,
        week_points,
        week_starters,
    }
}

/// A roster's season candidates: points and bye from the board, nothing for
/// a player the board does not know.
pub fn season_candidates(
    roster: &TeamRoster,
    board: &[BoardPlayer],
    board_index: &HashMap<String, usize>,
) -> Vec<Candidate> {
    roster
        .players
        .iter()
        .map(|p| {
            let on_board = board_index.get(&p.player_id).map(|&i| &board[i]);
            Candidate {
                player_id: p.player_id.clone(),
                name: p.name.clone(),
                position: p.position.clone(),
                points: on_board.map_or(0.0, |b| b.points),
                bye_week: on_board.and_then(|b| b.bye_week),
                injury: on_board.and_then(|b| b.injury_status.clone()),
            }
        })
        .collect()
}

/// One roster's candidates for a week, from the per-week points table. A
/// player with no row that week (bye, not projected) is left out; one who
/// is Out, on IR, suspended or doubtful is kept at zero, so a lineup that
/// starts him is scored honestly and the check can name his replacement.
pub fn week_candidates(
    season: &[Candidate],
    weekly_points: &HashMap<String, Vec<(u32, f64)>>,
    week: u32,
) -> Vec<Candidate> {
    season
        .iter()
        .filter_map(|c| {
            weekly_points
                .get(&c.player_id)
                .and_then(|weeks| weeks.iter().find(|(w, _)| *w == week))
                .map(|(_, pts)| Candidate {
                    points: if sidelined(c) { 0.0 } else { *pts },
                    ..c.clone()
                })
        })
        .collect()
}

/// Not playing this week, whatever the projection row says.
pub fn sidelined(c: &Candidate) -> bool {
    c.injury
        .as_deref()
        .is_some_and(crate::recommend::serious_injury)
}

/// Every team's projection, best season first. A drafted player missing
/// from the board (not projected) counts for nothing; one missing from the
/// week's rows (bye, no projection) is left out of that week's lineup.
pub fn standings(
    rosters: &[TeamRoster],
    board: &[BoardPlayer],
    board_index: &HashMap<String, usize>,
    weekly_points: &HashMap<String, Vec<(u32, f64)>>,
    week: u32,
    rules: &RosterRules,
) -> Vec<TeamProjection> {
    let mut out: Vec<TeamProjection> = rosters
        .iter()
        .map(|r| {
            let season = season_candidates(r, board, board_index);
            let this_week = week_candidates(&season, weekly_points, week);
            project(
                r.slot,
                r.display_name.clone(),
                &season,
                week,
                &this_week,
                rules,
            )
        })
        .collect();
    out.sort_by(|a, b| b.season.total_cmp(&a.season));
    out
}

/// One week where someone on the roster is on a bye.
#[derive(Debug, Clone, Serialize)]
pub struct ByeWeek {
    pub week: u32,
    /// Who is out, in roster order.
    pub out: Vec<String>,
    /// Best lineup that week, from season projections spread per game.
    pub points: f64,
    /// What the same roster scores in a week with nobody out.
    pub shortfall: f64,
    /// Starting slots nobody can fill that week.
    pub empty_slots: Vec<String>,
}

/// Starting slots a lineup leaves empty, in the league's order.
fn empty_slots(starters: &[Starter], rules: &RosterRules) -> Vec<String> {
    let mut filled: Vec<&str> = starters.iter().map(|s| s.slot.as_str()).collect();
    rules
        .slots()
        .iter()
        .filter(|s| !RosterRules::is_non_starting(s))
        .filter(|slot| {
            if let Some(i) = filled.iter().position(|f| f == slot) {
                filled.swap_remove(i);
                false
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

/// Where the roster is short before it happens: every week with a bye on
/// it, worst first. Season points spread evenly per game, the same
/// arithmetic the season total uses.
pub fn bye_weeks(season: &[Candidate], rules: &RosterRules) -> Vec<ByeWeek> {
    let per_game: Vec<Candidate> = season
        .iter()
        .map(|c| Candidate {
            points: c.points / f64::from(WEEKS),
            ..c.clone()
        })
        .collect();
    let (full, _) = best_lineup(&per_game, rules);
    let mut out: Vec<ByeWeek> = (1..=WEEKS)
        .filter_map(|week| {
            let away: Vec<String> = season
                .iter()
                .filter(|c| c.bye_week == Some(week))
                .map(|c| c.name.clone())
                .collect();
            if away.is_empty() {
                return None;
            }
            let active: Vec<Candidate> = per_game
                .iter()
                .filter(|c| c.bye_week != Some(week))
                .cloned()
                .collect();
            let (points, starters) = best_lineup(&active, rules);
            Some(ByeWeek {
                week,
                out: away,
                points,
                shortfall: full - points,
                empty_slots: empty_slots(&starters, rules),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.shortfall
            .total_cmp(&a.shortfall)
            .then(a.week.cmp(&b.week))
    });
    out
}

/// `bye_weeks` for a drafted roster, or nothing without one.
pub fn bye_weeks_for(
    roster: Option<&TeamRoster>,
    board: &[BoardPlayer],
    board_index: &HashMap<String, usize>,
    rules: &RosterRules,
) -> Vec<ByeWeek> {
    roster.map_or_else(Vec::new, |r| {
        bye_weeks(&season_candidates(r, board, board_index), rules)
    })
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
            &["QB", "RB", "WR", "FLEX", "BN"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn dedicated_slots_first_then_flex_takes_the_best_of_the_rest() {
        let team = [
            c("qb", "QB", 300.0, None),
            c("rb1", "RB", 200.0, None),
            c("rb2", "RB", 180.0, None),
            c("wr1", "WR", 190.0, None),
            c("wr2", "WR", 170.0, None),
        ];
        let (total, lineup) = best_lineup(&team, &rules());
        let slots: Vec<(&str, &str)> = lineup
            .iter()
            .map(|s| (s.slot.as_str(), s.player_id.as_str()))
            .collect();
        assert_eq!(
            slots,
            vec![("QB", "qb"), ("RB", "rb1"), ("WR", "wr1"), ("FLEX", "rb2")]
        );
        assert_eq!(total, 300.0 + 200.0 + 190.0 + 180.0);
    }

    #[test]
    fn an_empty_slot_is_simply_empty() {
        let team = [c("rb1", "RB", 200.0, None)];
        let (total, lineup) = best_lineup(&team, &rules());
        assert_eq!(lineup.len(), 1);
        assert_eq!(total, 200.0);
    }

    #[test]
    fn a_bye_week_costs_the_difference_to_the_next_man_up() {
        // One QB, on a bye in week 7: that week scores nothing at QB.
        let with_bye = [c("qb", "QB", 340.0, Some(7)), c("rb", "RB", 170.0, None)];
        let no_bye = [c("qb", "QB", 340.0, None), c("rb", "RB", 170.0, None)];
        let a = season_points(&with_bye, &rules());
        let b = season_points(&no_bye, &rules());
        assert!((b - a - 340.0 / 17.0).abs() < 1e-9, "{a} vs {b}");
        // And a backup on a different bye covers it.
        let covered = [
            c("qb", "QB", 340.0, Some(7)),
            c("qb2", "QB", 255.0, Some(9)),
            c("rb", "RB", 170.0, None),
        ];
        let cvd = season_points(&covered, &rules());
        assert!(cvd > a && cvd < b + 1e-9, "{a} < {cvd} <= {b}");
    }

    #[test]
    fn full_strength_never_exceeds_the_bye_adjusted_season_by_less_than_zero() {
        let team = [
            c("qb", "QB", 340.0, Some(5)),
            c("rb1", "RB", 200.0, Some(5)),
            c("rb2", "RB", 150.0, Some(11)),
            c("wr", "WR", 190.0, Some(5)),
        ];
        let p = project(3, Some("Me".into()), &team, 1, &[], &rules());
        assert!(p.season <= p.full_strength + 1e-9);
        assert_eq!(p.starters.len(), 4);
        assert_eq!(p.week_points, 0.0, "no rows for the week: nothing to start");
    }

    #[test]
    fn the_week_lineup_is_built_from_the_week_rows_not_the_season() {
        let season = [c("qb", "QB", 340.0, None), c("rb", "RB", 200.0, None)];
        // This week the QB sits (no row) and the back has a soft matchup.
        let week = [c("rb", "RB", 19.5, None)];
        let p = project(3, None, &season, 4, &week, &rules());
        assert_eq!(p.week, 4);
        assert_eq!(p.week_points, 19.5);
        assert_eq!(p.week_starters.len(), 1);
        assert_eq!(p.starters.len(), 2, "the season lineup is untouched");
    }

    #[test]
    fn a_sidelined_player_scores_nothing_for_the_week_but_stays_listed() {
        let mut out = c("wr1", "WR", 170.0, None);
        out.injury = Some("Out".into());
        let fine = c("wr2", "WR", 150.0, None);
        let weekly = HashMap::from([
            ("wr1".to_string(), vec![(3, 12.0)]),
            ("wr2".to_string(), vec![(3, 9.0)]),
        ]);
        let week = week_candidates(&[out, fine], &weekly, 3);
        assert_eq!(
            week.len(),
            2,
            "still on the list, so a set lineup can be scored"
        );
        assert_eq!(week[0].points, 0.0);
        assert_eq!(week[1].points, 9.0);
        // Questionable is not sidelined: through August it is on half the league.
        let mut q = c("wr3", "WR", 100.0, None);
        q.injury = Some("Questionable".into());
        assert!(!sidelined(&q));
    }

    #[test]
    fn bye_weeks_are_ranked_by_what_they_cost_and_name_an_unfillable_slot() {
        // QB, RB, WR, FLEX. One QB (bye 7), two backs (byes 5 and 5), two
        // receivers (bye 9 and none).
        let rules = RosterRules::new(
            &["QB", "RB", "WR", "FLEX", "BN"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        let team = [
            c("qb", "QB", 340.0, Some(7)),
            c("rb1", "RB", 200.0, Some(5)),
            c("rb2", "RB", 150.0, Some(5)),
            c("wr1", "WR", 190.0, Some(9)),
            c("wr2", "WR", 120.0, None),
        ];
        let weeks = bye_weeks(&team, &rules);
        let order: Vec<u32> = weeks.iter().map(|w| w.week).collect();
        // Week 7 loses the only QB (-340/17). Week 5 loses both backs: the
        // RB slot empties (-200/17) and wr2 takes the flex from rb2
        // (-30/17). Week 9 loses a receiver, covered by wr2 (-70/17).
        assert_eq!(order, vec![7, 5, 9]);
        assert_eq!(weeks[0].empty_slots, vec!["QB"]);
        assert!((weeks[0].shortfall - 340.0 / 17.0).abs() < 1e-9);
        assert_eq!(weeks[1].out, vec!["rb1", "rb2"]);
        assert_eq!(weeks[1].empty_slots, vec!["RB"], "{:?}", weeks[1]);
        assert!((weeks[1].shortfall - 230.0 / 17.0).abs() < 1e-9);
        assert!(weeks[2].empty_slots.is_empty());
        assert!((weeks[2].shortfall - 70.0 / 17.0).abs() < 1e-9);
    }
}
