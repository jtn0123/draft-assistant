//! Trades worth offering.
//!
//! A trade is only worth proposing if the other manager would plausibly accept
//! it, so every candidate here has to clear the same bar on both sides: the
//! swap must improve each team's best starting lineup. That naturally surfaces
//! the classic positional-surplus trade — my spare RB for your spare TE — and
//! naturally suppresses lopsided offers nobody would take.

use crate::roster::RosterRules;
use crate::season_lineup::{optimal_lineup, Candidate};
use serde::Serialize;

/// Most one-for-one offers to report.
const MAX_TRADES: usize = 4;
/// Ignore anything that moves a lineup by less than this per week — inside the
/// noise of a weekly projection, and not worth the message.
const MIN_EDGE: f64 = 0.5;

#[derive(Debug, Clone, Serialize)]
pub struct TradeIdea {
    pub roster_id: u32,
    pub partner: String,
    /// The player I would receive.
    pub get_id: String,
    pub get_name: String,
    pub get_position: String,
    /// The player I would give up.
    pub give_id: String,
    pub give_name: String,
    pub give_position: String,
    /// My weekly lineup improvement.
    pub my_edge: f64,
    /// Their weekly lineup improvement — positive, or they would decline.
    pub their_edge: f64,
    pub note: String,
}

fn lineup_total(rules: &RosterRules, candidates: &[Candidate]) -> f64 {
    optimal_lineup(rules, candidates)
        .iter()
        .map(|s| s.points)
        .sum()
}

/// Swap `out_id` for `incoming` and return the resulting best-lineup total.
fn total_after_swap(
    rules: &RosterRules,
    roster: &[Candidate],
    out_id: &str,
    incoming: &Candidate,
) -> f64 {
    let mut next: Vec<Candidate> = roster
        .iter()
        .filter(|c| c.player_id != out_id)
        .cloned()
        .collect();
    next.push(incoming.clone());
    lineup_total(rules, &next)
}

/// A rival roster to evaluate against.
pub struct TradePartner<'a> {
    pub roster_id: u32,
    pub name: String,
    pub candidates: &'a [Candidate],
}

/// Find one-for-one swaps that improve both sides' starting lineups.
pub fn trade_ideas(
    rules: &RosterRules,
    mine: &[Candidate],
    partners: &[TradePartner],
    describe: &impl Fn(&str) -> (String, String),
) -> Vec<TradeIdea> {
    let my_baseline = lineup_total(rules, mine);
    let mut ideas: Vec<TradeIdea> = Vec::new();

    for partner in partners {
        let their_baseline = lineup_total(rules, partner.candidates);
        for theirs in partner.candidates {
            for ours in mine {
                let my_after = total_after_swap(rules, mine, &ours.player_id, theirs);
                let my_edge = my_after - my_baseline;
                if my_edge < MIN_EDGE {
                    continue;
                }
                let their_after =
                    total_after_swap(rules, partner.candidates, &theirs.player_id, ours);
                let their_edge = their_after - their_baseline;
                if their_edge < MIN_EDGE {
                    continue;
                }
                let (get_name, get_position) = describe(&theirs.player_id);
                let (give_name, give_position) = describe(&ours.player_id);
                ideas.push(TradeIdea {
                    roster_id: partner.roster_id,
                    partner: partner.name.clone(),
                    get_id: theirs.player_id.clone(),
                    get_name,
                    get_position,
                    give_id: ours.player_id.clone(),
                    give_name,
                    give_position,
                    my_edge,
                    their_edge,
                    note: format!(
                        "{} gains {:+.1}/wk too \u{2014} both sides start a better lineup",
                        partner.name, their_edge
                    ),
                });
            }
        }
    }

    // Best for me first; break ties by what is most attractive to them, which
    // is what makes an offer likely to be accepted.
    ideas.sort_by(|a, b| {
        b.my_edge
            .total_cmp(&a.my_edge)
            .then_with(|| b.their_edge.total_cmp(&a.their_edge))
    });
    // At most one idea per partner: four variations on the same trade with the
    // same manager is noise, not four options.
    let mut seen: Vec<u32> = Vec::new();
    ideas.retain(|idea| {
        if seen.contains(&idea.roster_id) {
            false
        } else {
            seen.push(idea.roster_id);
            true
        }
    });
    ideas.truncate(MAX_TRADES);
    ideas
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

    fn describe(id: &str) -> (String, String) {
        let position = if id.starts_with("rb") {
            "RB"
        } else if id.starts_with("te") {
            "TE"
        } else {
            "WR"
        };
        (id.to_uppercase(), position.to_string())
    }

    #[test]
    fn positional_surplus_trades_are_found_and_help_both_sides() {
        let rules = rules(&["RB", "TE", "BN", "BN"]);
        // I have two good RBs and a bad TE; they have two good TEs and a bad RB.
        let mine = vec![
            candidate("rb1", "RB", 20.0),
            candidate("rb2", "RB", 18.0),
            candidate("te1", "TE", 4.0),
        ];
        let theirs = vec![
            candidate("te2", "TE", 17.0),
            candidate("te3", "TE", 15.0),
            candidate("rb3", "RB", 5.0),
        ];
        let ideas = trade_ideas(
            &rules,
            &mine,
            &[TradePartner {
                roster_id: 2,
                name: "punt_god".into(),
                candidates: &theirs,
            }],
            &describe,
        );
        assert_eq!(ideas.len(), 1);
        let idea = &ideas[0];
        assert_eq!(idea.give_id, "rb2");
        assert!(idea.get_id == "te3" || idea.get_id == "te2");
        assert!(idea.my_edge > 0.0 && idea.their_edge > 0.0);
    }

    #[test]
    fn a_swap_that_only_helps_me_is_never_offered() {
        let rules = rules(&["RB", "BN"]);
        let mine = vec![candidate("rb1", "RB", 5.0)];
        let theirs = vec![candidate("rb9", "RB", 25.0)];
        let ideas = trade_ideas(
            &rules,
            &mine,
            &[TradePartner {
                roster_id: 2,
                name: "them".into(),
                candidates: &theirs,
            }],
            &describe,
        );
        assert!(ideas.is_empty(), "{ideas:?}");
    }

    #[test]
    fn each_partner_contributes_at_most_one_idea() {
        let rules = rules(&["RB", "TE", "BN", "BN"]);
        let mine = vec![
            candidate("rb1", "RB", 20.0),
            candidate("rb2", "RB", 18.0),
            candidate("rb4", "RB", 17.0),
            candidate("te1", "TE", 3.0),
        ];
        let theirs = vec![
            candidate("te2", "TE", 17.0),
            candidate("te3", "TE", 16.0),
            candidate("rb3", "RB", 4.0),
        ];
        let partners = [
            TradePartner {
                roster_id: 2,
                name: "a".into(),
                candidates: &theirs,
            },
            TradePartner {
                roster_id: 3,
                name: "b".into(),
                candidates: &theirs,
            },
        ];
        let ideas = trade_ideas(&rules, &mine, &partners, &describe);
        assert_eq!(ideas.len(), 2);
        assert_ne!(ideas[0].roster_id, ideas[1].roster_id);
    }

    #[test]
    fn negligible_edges_are_filtered_out() {
        let rules = rules(&["RB", "BN"]);
        let mine = vec![candidate("rb1", "RB", 10.0)];
        let theirs = vec![candidate("rb2", "RB", 10.1)];
        let ideas = trade_ideas(
            &rules,
            &mine,
            &[TradePartner {
                roster_id: 2,
                name: "them".into(),
                candidates: &theirs,
            }],
            &describe,
        );
        assert!(ideas.is_empty());
    }
}
