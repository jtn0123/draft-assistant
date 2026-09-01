//! Trades worth offering.
//!
//! A trade is only worth proposing if the other manager would plausibly accept
//! it, so every candidate here has to clear the same bar on both sides: the
//! swap must improve each team's best starting lineup. That naturally surfaces
//! the classic positional-surplus trade — my spare RB for your spare TE — and
//! naturally suppresses lopsided offers nobody would take.

use crate::roster::RosterRules;
use crate::season_lineup::{lineup_total, Candidate};
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

/// Swap `out_id` for `incoming` and return the resulting best-lineup total.
///
/// `scratch` is reused across calls so the inner loop is not reallocating a
/// roster-sized vector thousands of times per refresh.
fn total_after_swap(
    rules: &RosterRules,
    roster: &[Candidate],
    out_id: &str,
    incoming: &Candidate,
    scratch: &mut Vec<Candidate>,
) -> f64 {
    scratch.clear();
    scratch.extend(roster.iter().filter(|c| c.player_id != out_id).cloned());
    scratch.push(incoming.clone());
    lineup_total(rules, scratch)
}

/// What a roster loses by giving up each of its players: the drop in its best
/// lineup total. Indexed alongside `roster`.
///
/// This is what makes the search affordable. Swapping `out` for `incoming`
/// changes the total by at most `incoming.points - cost_of_losing[out]` —
/// adding one player can never add more than that player's points, and losing
/// `out` always costs at least this much. So any pair whose bound falls short
/// of `MIN_EDGE` cannot produce an idea, and is skipped without solving a
/// single lineup.
fn cost_of_losing(rules: &RosterRules, roster: &[Candidate], baseline: f64) -> Vec<f64> {
    let mut scratch: Vec<Candidate> = Vec::with_capacity(roster.len());
    roster
        .iter()
        .map(|player| {
            scratch.clear();
            scratch.extend(
                roster
                    .iter()
                    .filter(|c| c.player_id != player.player_id)
                    .cloned(),
            );
            baseline - lineup_total(rules, &scratch)
        })
        .collect()
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
    let my_loss = cost_of_losing(rules, mine, my_baseline);
    let mut ideas: Vec<TradeIdea> = Vec::new();
    let mut scratch: Vec<Candidate> = Vec::new();

    for partner in partners {
        let their_baseline = lineup_total(rules, partner.candidates);
        let their_loss = cost_of_losing(rules, partner.candidates, their_baseline);
        for (their_index, theirs) in partner.candidates.iter().enumerate() {
            for (my_index, ours) in mine.iter().enumerate() {
                // Cheap bound first: most pairs die here without a solve.
                if theirs.points - my_loss[my_index] < MIN_EDGE {
                    continue;
                }
                let my_after = total_after_swap(rules, mine, &ours.player_id, theirs, &mut scratch);
                let my_edge = my_after - my_baseline;
                if my_edge < MIN_EDGE {
                    continue;
                }
                if ours.points - their_loss[their_index] < MIN_EDGE {
                    continue;
                }
                let their_after = total_after_swap(
                    rules,
                    partner.candidates,
                    &theirs.player_id,
                    ours,
                    &mut scratch,
                );
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
    /// Brute force: every pair, both totals solved, no bounds. The pruned
    /// search must agree with this exactly.
    fn ideas_by_brute_force(
        rules: &RosterRules,
        mine: &[Candidate],
        partners: &[TradePartner],
    ) -> Vec<(u32, String, String)> {
        let my_baseline = lineup_total(rules, mine);
        let mut found = Vec::new();
        for partner in partners {
            let their_baseline = lineup_total(rules, partner.candidates);
            for theirs in partner.candidates {
                for ours in mine {
                    let mut scratch = Vec::new();
                    let my_edge =
                        total_after_swap(rules, mine, &ours.player_id, theirs, &mut scratch)
                            - my_baseline;
                    if my_edge < MIN_EDGE {
                        continue;
                    }
                    let their_edge = total_after_swap(
                        rules,
                        partner.candidates,
                        &theirs.player_id,
                        ours,
                        &mut scratch,
                    ) - their_baseline;
                    if their_edge < MIN_EDGE {
                        continue;
                    }
                    found.push((
                        partner.roster_id,
                        theirs.player_id.clone(),
                        ours.player_id.clone(),
                    ));
                }
            }
        }
        found
    }

    #[test]
    fn pruning_never_hides_a_trade_the_full_search_would_find() {
        // A spread of rosters and point distributions, deterministic so a
        // failure is reproducible.
        let rules = RosterRules::new(
            &["QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "BN", "BN", "BN"]
                .iter()
                .map(|s| (*s).to_string())
                .collect::<Vec<_>>(),
        );
        let positions = ["QB", "RB", "WR", "TE"];
        let mut seed = 12_345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as f64 % 25.0
        };

        for trial in 0..40 {
            let mine: Vec<Candidate> = (0..10)
                .map(|i| Candidate {
                    player_id: format!("mine-{trial}-{i}"),
                    position: positions[i % positions.len()].to_string(),
                    points: next(),
                })
                .collect();
            let theirs: Vec<Candidate> = (0..10)
                .map(|i| Candidate {
                    player_id: format!("theirs-{trial}-{i}"),
                    position: positions[i % positions.len()].to_string(),
                    points: next(),
                })
                .collect();
            let partners = vec![TradePartner {
                roster_id: 2,
                name: "Rival".into(),
                candidates: &theirs,
            }];

            let brute = ideas_by_brute_force(&rules, &mine, &partners);
            let pruned = trade_ideas(&rules, &mine, &partners, &|id| {
                (id.to_string(), "RB".to_string())
            });

            // trade_ideas keeps one idea per partner, so the check is that
            // whatever it kept is a pair brute force also found, and that it
            // finds something whenever brute force does.
            assert_eq!(
                pruned.is_empty(),
                brute.is_empty(),
                "trial {trial}: pruned {} vs brute {}",
                pruned.len(),
                brute.len()
            );
            for idea in &pruned {
                assert!(
                    brute.contains(&(idea.roster_id, idea.get_id.clone(), idea.give_id.clone())),
                    "trial {trial}: pruned search invented {:?}",
                    idea.get_id
                );
            }
        }
    }

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
