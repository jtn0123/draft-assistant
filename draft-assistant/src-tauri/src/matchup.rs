//! This week: is the lineup set on Sleeper the best one, and how does it
//! stack up against the opponent?
//!
//! Sleeper's API is read-only, so the app cannot set a lineup. What it can do
//! is say, in slot order, "Shakir over Gainwell in FLEX, +2.1" and "DEF is
//! empty" — which on draft night it did not, and the user finished with an
//! empty defense slot and a running back starting over a better receiver.

use crate::draft::TeamRoster;
use crate::engine::LoadedLeague;
use crate::lineup::{best_lineup, Candidate, Starter};
use crate::roster::RosterRules;
use crate::scoring::norm_cdf;
use crate::sleeper::Matchup;
use serde::Serialize;
use std::collections::HashMap;

/// Week-to-week spread of a fantasy player around his projection, as a
/// fraction of it. Wide, because a weekly projection is a mean over boom and
/// bust games; the team-level sigma this produces (about 20 points for nine
/// starters) matches what real fantasy scores do.
const PLAYER_CV: f64 = 0.5;

#[derive(Debug, Clone, Serialize)]
pub struct LineupChange {
    pub slot: String,
    /// Who is set there now. `None` for an empty slot.
    pub out: Option<Starter>,
    pub in_: Starter,
    pub gain: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineupCheck {
    /// The lineup as set on Sleeper, scored on this week's projections.
    pub set_points: f64,
    /// The best lineup from the same roster.
    pub best_points: f64,
    /// Slot by slot, what to change. Empty when the set lineup is the best.
    pub changes: Vec<LineupChange>,
    /// Starting slots with nobody in them.
    pub empty_slots: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchupPreview {
    pub opponent_slot: u32,
    pub opponent_name: Option<String>,
    /// My best lineup this week.
    pub my_points: f64,
    /// The opponent's lineup as set, or their best if none is set.
    pub opponent_points: f64,
    pub margin: f64,
    /// P(my score > theirs) with both spread `PLAYER_CV` per starter.
    pub win_probability: f64,
    pub my_starters: Vec<Starter>,
    pub opponent_starters: Vec<Starter>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThisWeek {
    pub week: u32,
    pub lineup: Option<LineupCheck>,
    pub matchup: Option<MatchupPreview>,
}

/// Starting slots in the league's order — what Sleeper's `starters` array
/// lines up against.
fn starting_slots(rules: &RosterRules) -> Vec<&str> {
    rules
        .slots()
        .iter()
        .map(String::as_str)
        .filter(|s| !RosterRules::is_non_starting(s))
        .collect()
}

/// Score a lineup as set: each starter id resolved against the week's
/// candidates, in slot order. Unknown ids (not projected this week) and
/// `"0"` score nothing.
fn set_lineup(
    starters: &[String],
    week: &[Candidate],
    rules: &RosterRules,
) -> Vec<Option<Starter>> {
    starting_slots(rules)
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let id = starters.get(i)?;
            if id == "0" {
                return None;
            }
            week.iter().find(|c| &c.player_id == id).map(|c| Starter {
                slot: (*slot).to_string(),
                player_id: c.player_id.clone(),
                name: c.name.clone(),
                position: c.position.clone(),
                points: c.points,
                injury: c.injury.clone(),
            })
        })
        .collect()
}

pub fn lineup_check(starters: &[String], week: &[Candidate], rules: &RosterRules) -> LineupCheck {
    let set = set_lineup(starters, week, rules);
    let set_points: f64 = set.iter().flatten().map(|s| s.points).sum();
    let (best_points, best) = best_lineup(week, rules);
    let slots = starting_slots(rules);
    let mut changes = Vec::new();
    let mut empty_slots = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let now = set.get(i).cloned().flatten();
        // The best lineup's occupant of this slot index. Slots of one kind
        // are interchangeable, so match by slot name and consume in order.
        let want = best
            .iter()
            .filter(|s| s.slot == *slot)
            .nth(slots[..i].iter().filter(|x| **x == *slot).count());
        let Some(want) = want else {
            // Nobody on the roster can fill it at all — the draft-night
            // case: no defense drafted. Still empty, and the one thing the
            // best lineup cannot say on its own.
            if now.is_none() {
                empty_slots.push((*slot).to_string());
            }
            continue;
        };
        let same = now.as_ref().is_some_and(|n| n.player_id == want.player_id);
        // Also fine if the set lineup starts him elsewhere: a FLEX/WR swap
        // of the same two players is not a change worth reporting.
        let started_somewhere = set.iter().flatten().any(|n| n.player_id == want.player_id);
        if same || started_somewhere {
            continue;
        }
        let gain = want.points - now.as_ref().map_or(0.0, |n| n.points);
        if now.is_none() {
            empty_slots.push((*slot).to_string());
        }
        if gain > 0.05 {
            changes.push(LineupChange {
                slot: (*slot).to_string(),
                out: now,
                in_: want.clone(),
                gain,
            });
        }
    }
    LineupCheck {
        set_points,
        best_points,
        changes,
        empty_slots,
    }
}

fn team_sigma(starters: &[Starter]) -> f64 {
    starters
        .iter()
        .map(|s| (PLAYER_CV * s.points).powi(2))
        .sum::<f64>()
        .sqrt()
}

pub fn preview(
    my_week: &[Candidate],
    opponent: (u32, Option<String>, &[String], &[Candidate]),
    rules: &RosterRules,
) -> MatchupPreview {
    let (opponent_slot, opponent_name, their_set, their_week) = opponent;
    let (my_points, my_starters) = best_lineup(my_week, rules);
    let set: Vec<Starter> = set_lineup(their_set, their_week, rules)
        .into_iter()
        .flatten()
        .collect();
    let (opponent_points, opponent_starters) = if set.is_empty() {
        best_lineup(their_week, rules)
    } else {
        (set.iter().map(|s| s.points).sum(), set)
    };
    let margin = my_points - opponent_points;
    let sigma = (team_sigma(&my_starters).powi(2) + team_sigma(&opponent_starters).powi(2)).sqrt();
    let win_probability = if sigma > 0.0 {
        norm_cdf(margin / sigma)
    } else if margin > 0.0 {
        1.0
    } else {
        0.5
    };
    MatchupPreview {
        opponent_slot,
        opponent_name,
        my_points,
        opponent_points,
        margin,
        win_probability,
        my_starters,
        opponent_starters,
    }
}

/// The two rosters sharing my matchup id, if the week has pairings.
pub fn opponent_roster_id(matchups: &[Matchup], my_roster_id: u32) -> Option<u32> {
    let mine = matchups.iter().find(|m| m.roster_id == my_roster_id)?;
    let id = mine.matchup_id?;
    matchups
        .iter()
        .find(|m| m.matchup_id == Some(id) && m.roster_id != my_roster_id)
        .map(|m| m.roster_id)
}

/// Lineup check and matchup preview for `my_slot`, from this week's Sleeper
/// matchups. Rosters come from the draft (`rosters`), lineups from Sleeper.
pub fn this_week(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    my_slot: Option<u32>,
    week: u32,
) -> Option<ThisWeek> {
    let my_slot = my_slot?;
    let slot_to_roster: HashMap<u32, u32> = loaded
        .draft
        .slot_to_roster_id
        .as_ref()?
        .iter()
        .filter_map(|(s, r)| s.parse().ok().map(|s: u32| (s, *r)))
        .collect();
    let roster_of = |slot: u32| slot_to_roster.get(&slot).copied();
    let slot_of = |roster_id: u32| {
        slot_to_roster
            .iter()
            .find(|(_, r)| **r == roster_id)
            .map(|(s, _)| *s)
    };
    let my_roster_id = roster_of(my_slot)?;
    let candidates = |slot: u32| {
        rosters.get((slot - 1) as usize).map(|r| {
            let season = crate::lineup::season_candidates(r, &loaded.board, &loaded.board_index);
            crate::lineup::week_candidates(&season, &loaded.weekly_points, week)
        })
    };
    let mine = candidates(my_slot)?;
    let lineup = loaded
        .matchups
        .iter()
        .find(|m| m.roster_id == my_roster_id && !m.starters.is_empty())
        .map(|m| lineup_check(&m.starters, &mine, &loaded.roster_rules));
    let matchup = opponent_roster_id(&loaded.matchups, my_roster_id)
        .and_then(slot_of)
        .and_then(|opp_slot| {
            let theirs = candidates(opp_slot)?;
            let set: &[String] = loaded
                .matchups
                .iter()
                .find(|m| Some(m.roster_id) == roster_of(opp_slot))
                .map_or(&[], |m| m.starters.as_slice());
            let name = rosters
                .get((opp_slot - 1) as usize)
                .and_then(|r| r.display_name.clone());
            Some(preview(
                &mine,
                (opp_slot, name, set, &theirs),
                &loaded.roster_rules,
            ))
        });
    if lineup.is_none() && matchup.is_none() {
        return None;
    }
    Some(ThisWeek {
        week,
        lineup,
        matchup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, pos: &str, pts: f64) -> Candidate {
        Candidate {
            player_id: id.into(),
            name: id.into(),
            position: pos.into(),
            points: pts,
            bye_week: None,
            injury: None,
        }
    }

    fn rules() -> RosterRules {
        RosterRules::new(
            &["QB", "RB", "WR", "FLEX", "DEF", "BN"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn week() -> Vec<Candidate> {
        vec![
            c("qb", "QB", 20.0),
            c("rb1", "RB", 18.0),
            c("rb2", "RB", 9.0),
            c("wr1", "WR", 15.0),
            c("wr2", "WR", 11.0),
            c("def", "DEF", 7.0),
        ]
    }

    #[test]
    fn a_worse_flex_and_an_empty_slot_are_both_reported() {
        // Set: rb2 in FLEX over wr2, and no DEF.
        let set: Vec<String> = ["qb", "rb1", "wr1", "rb2", "0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let check = lineup_check(&set, &week(), &rules());
        assert_eq!(check.set_points, 62.0);
        assert_eq!(check.best_points, 71.0);
        assert_eq!(check.empty_slots, vec!["DEF"]);
        let changes: Vec<(&str, Option<&str>, &str)> = check
            .changes
            .iter()
            .map(|x| {
                (
                    x.slot.as_str(),
                    x.out.as_ref().map(|o| o.player_id.as_str()),
                    x.in_.player_id.as_str(),
                )
            })
            .collect();
        assert_eq!(
            changes,
            vec![("FLEX", Some("rb2"), "wr2"), ("DEF", None, "def")]
        );
        assert!((check.changes[0].gain - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_slot_nobody_can_fill_is_reported_empty_with_no_change_to_make() {
        // No DEF on the roster at all; the slot is set to "0".
        let roster: Vec<Candidate> = week().into_iter().filter(|c| c.position != "DEF").collect();
        let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "0"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let check = lineup_check(&set, &roster, &rules());
        assert_eq!(check.empty_slots, vec!["DEF"]);
        assert!(check.changes.is_empty(), "{:?}", check.changes);
    }

    #[test]
    fn the_best_lineup_set_needs_no_changes() {
        let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "def"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let check = lineup_check(&set, &week(), &rules());
        assert!(check.changes.is_empty(), "{:?}", check.changes);
        assert!(check.empty_slots.is_empty());
        assert_eq!(check.set_points, check.best_points);
    }

    #[test]
    fn the_same_players_in_swapped_slots_is_not_a_change() {
        // wr2 in WR, wr1 in FLEX: same nine points, different order.
        let set: Vec<String> = ["qb", "rb1", "wr2", "wr1", "def"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let check = lineup_check(&set, &week(), &rules());
        assert!(check.changes.is_empty(), "{:?}", check.changes);
    }

    #[test]
    fn a_projected_favourite_is_more_likely_to_win_and_a_tie_is_a_coin_flip() {
        let theirs = vec![
            c("tqb", "QB", 18.0),
            c("trb", "RB", 12.0),
            c("twr", "WR", 12.0),
            c("twr2", "WR", 8.0),
            c("tdef", "DEF", 6.0),
        ];
        let p = preview(&week(), (5, Some("Them".into()), &[], &theirs), &rules());
        assert_eq!(p.opponent_points, 56.0);
        assert!(p.margin > 0.0);
        assert!(
            p.win_probability > 0.5 && p.win_probability < 1.0,
            "{}",
            p.win_probability
        );
        let even = preview(&week(), (5, None, &[], &week()), &rules());
        assert!(
            (even.win_probability - 0.5).abs() < 1e-6,
            "{}",
            even.win_probability
        );
    }

    #[test]
    fn the_opponents_set_lineup_is_used_when_they_have_one() {
        // They benched their best back.
        let theirs = week();
        let set: Vec<String> = ["qb", "rb2", "wr1", "wr2", "def"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = preview(&week(), (5, None, &set, &theirs), &rules());
        assert_eq!(p.opponent_points, 62.0);
    }

    #[test]
    fn opponents_share_a_matchup_id() {
        let m = |roster_id: u32, matchup_id: u32| Matchup {
            roster_id,
            matchup_id: Some(matchup_id),
            starters: Vec::new(),
            players: Vec::new(),
            points: 0.0,
            players_points: Default::default(),
        };
        let ms = [m(1, 7), m(3, 7), m(2, 4)];
        assert_eq!(opponent_roster_id(&ms, 1), Some(3));
        assert_eq!(opponent_roster_id(&ms, 2), None);
    }

    #[test]
    fn an_out_starter_is_replaced_and_the_change_says_why() {
        let mut roster = week();
        // rb1 is set and Out; he scores nothing this week.
        roster[1].injury = Some("Out".into());
        roster[1].points = 0.0;
        let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "def"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let check = lineup_check(&set, &roster, &rules());
        assert_eq!(check.changes.len(), 1, "{:?}", check.changes);
        let ch = &check.changes[0];
        assert_eq!((ch.slot.as_str(), ch.in_.player_id.as_str()), ("RB", "rb2"));
        assert_eq!(
            ch.out.as_ref().and_then(|o| o.injury.as_deref()),
            Some("Out")
        );
        assert!((ch.gain - 9.0).abs() < 1e-9);
    }
}
