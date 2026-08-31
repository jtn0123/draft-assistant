//! Optimal lineups, and the gap between the optimal one and what is actually set.
//!
//! "Calls to make" in the season screen is exactly this diff: fill every
//! starting slot with the best eligible player, then report where that
//! disagrees with the roster's current starters.

use crate::roster::RosterRules;
use crate::weekly::WeeklyPoints;
use serde::Serialize;
use std::collections::HashSet;

/// One filled starting slot.
#[derive(Debug, Clone, Serialize)]
pub struct LineupSlot {
    /// Roster slot label: "QB", "RB", "FLEX", …
    pub slot: String,
    pub player_id: Option<String>,
    pub points: f64,
}

/// A player considered for a lineup slot.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub player_id: String,
    pub position: String,
    pub points: f64,
}

/// Fill the roster's starting slots with the highest-scoring eligible players.
///
/// Slots are filled most-constrained first (a plain "TE" before a "REC_FLEX"
/// before a "SUPER_FLEX"), which is what makes the greedy pass optimal here:
/// every slot's eligibility set is either disjoint from or a superset of the
/// ones filled before it, so taking the best available for a narrow slot can
/// never strand a wider slot with nothing to put in it.
pub fn optimal_lineup(rules: &RosterRules, candidates: &[Candidate]) -> Vec<LineupSlot> {
    let mut order: Vec<(usize, &String)> = rules
        .slots()
        .iter()
        .enumerate()
        .filter(|(_, slot)| !RosterRules::is_non_starting(slot))
        .collect();
    order.sort_by_key(|(index, slot)| {
        let width = RosterRules::flex_eligible(slot).map_or(1, <[&str]>::len);
        (width, *index)
    });

    let mut used: HashSet<&str> = HashSet::new();
    let mut filled: Vec<(usize, LineupSlot)> = Vec::new();
    for (index, slot) in order {
        let best = candidates
            .iter()
            .filter(|c| !used.contains(c.player_id.as_str()))
            .filter(|c| RosterRules::can_fill(slot, &c.position))
            .max_by(|a, b| a.points.total_cmp(&b.points));
        match best {
            Some(c) => {
                used.insert(c.player_id.as_str());
                filled.push((
                    index,
                    LineupSlot {
                        slot: slot.clone(),
                        player_id: Some(c.player_id.clone()),
                        points: c.points,
                    },
                ));
            }
            None => filled.push((
                index,
                LineupSlot {
                    slot: slot.clone(),
                    player_id: None,
                    points: 0.0,
                },
            )),
        }
    }
    // Back to the league's own slot order for display.
    filled.sort_by_key(|(index, _)| *index);
    filled.into_iter().map(|(_, slot)| slot).collect()
}

/// Total points of the best lineup this roster can field in one week.
pub fn optimal_points(
    rules: &RosterRules,
    player_ids: &[String],
    position_of: &impl Fn(&str) -> Option<String>,
    weekly: &WeeklyPoints,
    week: u32,
) -> f64 {
    let candidates = candidates_for(player_ids, position_of, weekly, week);
    optimal_lineup(rules, &candidates)
        .iter()
        .map(|s| s.points)
        .sum()
}

/// Build week-scored candidates for every player on a roster. Players with no
/// projection this week (bye, or unprojected) are still candidates at zero, so
/// a roster that is short at a position still reports the slot as filled-empty
/// rather than silently dropping it.
pub fn candidates_for(
    player_ids: &[String],
    position_of: &impl Fn(&str) -> Option<String>,
    weekly: &WeeklyPoints,
    week: u32,
) -> Vec<Candidate> {
    player_ids
        .iter()
        .filter_map(|id| {
            let position = position_of(id)?;
            Some(Candidate {
                player_id: id.clone(),
                position,
                points: weekly.get_or_zero(id, week),
            })
        })
        .collect()
}

/// A start/sit call: swap `out` for `in` at `slot` and gain `gain` points.
#[derive(Debug, Clone, Serialize)]
pub struct LineupCall {
    pub slot: String,
    pub player_in: String,
    pub player_in_id: String,
    pub player_in_team: Option<String>,
    pub player_out: String,
    pub player_out_id: String,
    pub gain: f64,
    pub why: String,
    /// One line of plain language for *why*, beyond the point difference:
    /// "he's on bye", "your starter is listed Out". Filled in by
    /// `season_calls`, which is where the injury and schedule data lives.
    #[serde(default)]
    pub reason: Option<String>,
    /// Epoch milliseconds by which the swap has to be made — the earlier of
    /// the two players' kickoffs. `None` when the scoreboard has no game for
    /// either of them, or when both have already started.
    #[serde(default)]
    pub locks_at_ms: Option<i64>,
}

/// Diff the optimal lineup against the starters actually set.
///
/// A call is "start X over Y". X is anyone in the optimal lineup who is not
/// starting at all; Y is a set starter who is not in the optimal lineup (or an
/// empty slot). Players who are optimal but already starting in a *different*
/// slot are never a call — moving someone between two slots they both qualify
/// for changes nothing — and, crucially, they never block a call either: if
/// the set lineup is a shuffled copy of the optimal one apart from a single
/// bench-worthy starter, that starter is still reported.
///
/// Incoming players are matched, best first, to the lowest-scoring outgoing
/// starter sitting in a slot they are eligible for (`eligible(slot, id)`), so
/// the slot named on the call is one the user can actually put them in.
pub fn calls_from_diff(
    optimal: &[LineupSlot],
    current: &[LineupSlot],
    eligible: &impl Fn(&str, &str) -> bool,
    describe: &impl Fn(&str) -> (String, Option<String>),
    reason: &impl Fn(&str, &str, &str) -> String,
) -> Vec<LineupCall> {
    let optimal_ids: HashSet<&str> = optimal
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();
    let current_ids: HashSet<&str> = current
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();

    let mut incoming: Vec<&LineupSlot> = optimal
        .iter()
        .filter(|s| {
            s.player_id
                .as_deref()
                .is_some_and(|id| !current_ids.contains(id))
        })
        .collect();
    incoming.sort_by(|a, b| b.points.total_cmp(&a.points));

    // Set starters who should not be starting, plus every empty set slot.
    let mut outgoing: Vec<&LineupSlot> = current
        .iter()
        .filter(|s| {
            s.player_id
                .as_deref()
                .is_none_or(|id| !optimal_ids.contains(id))
        })
        .collect();

    let mut calls = Vec::new();
    for best in incoming {
        let Some(best_id) = best.player_id.as_deref() else {
            continue;
        };
        let pick = |fits: &dyn Fn(&LineupSlot) -> bool| {
            outgoing
                .iter()
                .enumerate()
                .filter(|(_, out)| fits(out))
                .min_by(|(_, a), (_, b)| a.points.total_cmp(&b.points))
                .map(|(i, _)| i)
        };
        let Some(index) = pick(&|out| eligible(&out.slot, best_id)).or_else(|| pick(&|_| true))
        else {
            break;
        };
        let now = outgoing.remove(index);
        let now_id = now.player_id.as_deref().unwrap_or("");
        let gain = best.points - now.points;
        if gain <= 0.05 {
            continue;
        }
        let (in_name, in_team) = describe(best_id);
        let (out_name, _) = if now_id.is_empty() {
            ("an empty slot".to_string(), None)
        } else {
            describe(now_id)
        };
        calls.push(LineupCall {
            slot: now.slot.clone(),
            player_in: in_name,
            player_in_id: best_id.to_string(),
            player_in_team: in_team,
            player_out: out_name,
            player_out_id: now_id.to_string(),
            gain,
            why: reason(&now.slot, best_id, now_id),
            reason: None,
            locks_at_ms: None,
        });
    }
    calls.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(slots: &[&str]) -> RosterRules {
        RosterRules::new(&slots.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    fn candidate(id: &str, position: &str, points: f64) -> Candidate {
        Candidate {
            player_id: id.into(),
            position: position.into(),
            points,
        }
    }

    fn ids(lineup: &[LineupSlot]) -> Vec<&str> {
        lineup
            .iter()
            .map(|s| s.player_id.as_deref().unwrap_or("-"))
            .collect()
    }

    #[test]
    fn flex_takes_the_leftover_rather_than_stealing_a_dedicated_slot() {
        let rules = rules(&["RB", "WR", "FLEX", "BN"]);
        let lineup = optimal_lineup(
            &rules,
            &[
                candidate("rb1", "RB", 20.0),
                candidate("rb2", "RB", 18.0),
                candidate("wr1", "WR", 15.0),
            ],
        );
        // The RB slot must not be left empty just because FLEX came first.
        assert_eq!(ids(&lineup), vec!["rb1", "wr1", "rb2"]);
    }

    #[test]
    fn superflex_is_filled_after_narrower_slots() {
        let rules = rules(&["SUPER_FLEX", "QB", "RB"]);
        let lineup = optimal_lineup(
            &rules,
            &[
                candidate("qb1", "QB", 25.0),
                candidate("qb2", "QB", 22.0),
                candidate("rb1", "RB", 20.0),
            ],
        );
        // Displayed in league order: SUPER_FLEX, QB, RB.
        assert_eq!(ids(&lineup), vec!["qb2", "qb1", "rb1"]);
    }

    #[test]
    fn short_rosters_report_an_empty_slot_instead_of_dropping_it() {
        let rules = rules(&["QB", "TE"]);
        let lineup = optimal_lineup(&rules, &[candidate("qb1", "QB", 25.0)]);
        assert_eq!(ids(&lineup), vec!["qb1", "-"]);
        assert_eq!(lineup.len(), 2);
    }

    fn describe(id: &str) -> (String, Option<String>) {
        (id.to_uppercase(), Some("PIT".into()))
    }

    fn reason(_slot: &str, _a: &str, _b: &str) -> String {
        "because".into()
    }

    fn any(_slot: &str, _id: &str) -> bool {
        true
    }

    fn slot(slot: &str, id: Option<&str>, points: f64) -> LineupSlot {
        LineupSlot {
            slot: slot.into(),
            player_id: id.map(str::to_string),
            points,
        }
    }

    #[test]
    fn a_shuffled_lineup_still_reports_the_one_starter_who_should_sit() {
        // Real week-1 case: WR and a FLEX are swapped relative to the optimal
        // lineup (harmless), but Pollard is set at FLEX over Downs on the
        // bench. Slot-by-slot pairing hid that call entirely.
        let optimal = vec![
            slot("WR", Some("watson"), 14.3),
            slot("FLEX", Some("wilson"), 14.0),
            slot("FLEX", Some("downs"), 12.8),
        ];
        let current = vec![
            slot("WR", Some("wilson"), 14.0),
            slot("FLEX", Some("pollard"), 9.1),
            slot("FLEX", Some("watson"), 14.3),
        ];
        let eligible = |slot: &str, id: &str| slot == "FLEX" || id != "downs";
        let calls = calls_from_diff(&optimal, &current, &eligible, &describe, &reason);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].player_in, "DOWNS");
        assert_eq!(calls[0].player_out, "POLLARD");
        assert_eq!(calls[0].slot, "FLEX");
        assert!((calls[0].gain - 3.7).abs() < 1e-9);
    }

    #[test]
    fn incoming_players_prefer_a_slot_they_can_fill() {
        // A TE coming in must displace the weak TE, not the even weaker RB
        // sitting in a slot the TE cannot occupy.
        let optimal = vec![
            slot("RB", Some("rb"), 10.0),
            slot("TE", Some("te_good"), 11.0),
        ];
        let current = vec![
            slot("RB", Some("rb_weak"), 4.0),
            slot("TE", Some("te_weak"), 6.0),
        ];
        let eligible = |slot: &str, id: &str| {
            (slot == "TE" && id.starts_with("te")) || (slot == "RB" && id.starts_with("rb"))
        };
        let calls = calls_from_diff(&optimal, &current, &eligible, &describe, &reason);
        let te = calls
            .iter()
            .find(|c| c.player_in == "TE_GOOD")
            .expect("te call");
        assert_eq!(te.player_out, "TE_WEAK");
        assert_eq!(te.slot, "TE");
    }

    #[test]
    fn moving_a_starter_between_eligible_slots_is_not_a_call() {
        let optimal = vec![
            LineupSlot {
                slot: "RB".into(),
                player_id: Some("a".into()),
                points: 10.0,
            },
            LineupSlot {
                slot: "FLEX".into(),
                player_id: Some("b".into()),
                points: 9.0,
            },
        ];
        let current = vec![
            LineupSlot {
                slot: "RB".into(),
                player_id: Some("b".into()),
                points: 9.0,
            },
            LineupSlot {
                slot: "FLEX".into(),
                player_id: Some("a".into()),
                points: 10.0,
            },
        ];
        assert!(calls_from_diff(&optimal, &current, &any, &describe, &reason).is_empty());
    }

    #[test]
    fn benching_a_starter_for_a_better_one_is_a_call_sorted_by_gain() {
        let optimal = vec![
            LineupSlot {
                slot: "RB".into(),
                player_id: Some("good".into()),
                points: 18.0,
            },
            LineupSlot {
                slot: "WR".into(),
                player_id: Some("best".into()),
                points: 20.0,
            },
        ];
        let current = vec![
            LineupSlot {
                slot: "RB".into(),
                player_id: Some("bad".into()),
                points: 16.0,
            },
            LineupSlot {
                slot: "WR".into(),
                player_id: Some("worse".into()),
                points: 12.0,
            },
        ];
        let calls = calls_from_diff(&optimal, &current, &any, &describe, &reason);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].player_in, "BEST");
        assert!((calls[0].gain - 8.0).abs() < 1e-9);
        assert_eq!(calls[1].player_in, "GOOD");
    }

    #[test]
    fn an_optimal_lineup_totals_a_positive_zero_gain() {
        // f64's additive identity is -0.0, which would serialise as "-0.0" and
        // read as a negative number of points left on the table.
        let lineup = vec![LineupSlot {
            slot: "QB".into(),
            player_id: Some("a".into()),
            points: 10.0,
        }];
        let calls = calls_from_diff(&lineup, &lineup, &any, &describe, &reason);
        assert!(calls.is_empty());
        let total: f64 = calls.iter().map(|c| c.gain).sum::<f64>() + 0.0;
        assert!(total.is_sign_positive(), "expected +0.0, got {total}");
    }

    #[test]
    fn an_empty_starting_slot_is_reported_as_a_call() {
        let optimal = vec![LineupSlot {
            slot: "TE".into(),
            player_id: Some("te1".into()),
            points: 11.0,
        }];
        let current = vec![LineupSlot {
            slot: "TE".into(),
            player_id: None,
            points: 0.0,
        }];
        let calls = calls_from_diff(&optimal, &current, &any, &describe, &reason);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].player_out, "an empty slot");
    }
}
