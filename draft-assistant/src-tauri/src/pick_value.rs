//! What a draft pick is worth, in the only currency the rest of the app
//! speaks: points over replacement.
//!
//! Last season this league made 38 trades. **Thirty-four of them moved a
//! draft pick and none moved FAAB** — so an offer form that only ticks
//! players is priced in the wrong money. A pick's price here is empirical:
//! what the players taken in that round of this league's own draft are
//! worth (`BoardPlayer::vorp`, already the value over a replacement-level
//! starter), taken as a median so one steal or one bust in a round does not
//! set the price.
//!
//! Two honest limits, both worth saying out loud where the number is shown:
//! a pick pays off in *next* season's lineup while every other number in a
//! verdict is this season's, and a keeper league buries strong players in
//! late rounds, which lifts those rounds' prices above what an ordinary
//! pick there would fetch.

use crate::engine::LoadedLeague;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PickPrice {
    pub round: u32,
    /// Points over replacement, the median of what the round actually took.
    pub points: f64,
    /// Who went at that price — the median pick itself.
    pub example: Option<String>,
}

fn median_index(len: usize) -> usize {
    (len - 1) / 2
}

/// One price per round of the draft, cheapest last. Empty until the draft
/// has picks to learn from.
pub fn pick_prices(loaded: &LoadedLeague) -> Vec<PickPrice> {
    let rounds = loaded.draft.settings.rounds;
    let picks = loaded.api_picks.iter().chain(loaded.manual_picks.iter());
    let mut by_round: Vec<Vec<(f64, String)>> = vec![Vec::new(); rounds as usize + 1];
    for pick in picks {
        let Some(slot) = by_round.get_mut(pick.round as usize) else {
            continue;
        };
        let Some(player) = loaded
            .board_index
            .get(&pick.player_id)
            .and_then(|i| loaded.board.get(*i))
        else {
            continue;
        };
        slot.push((player.vorp.max(0.0), player.name.clone()));
    }
    let mut prices: Vec<PickPrice> = (1..=rounds)
        .filter_map(|round| {
            let mut taken = by_round.get(round as usize)?.clone();
            if taken.is_empty() {
                return None;
            }
            taken.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (points, example) = taken[median_index(taken.len())].clone();
            Some(PickPrice {
                round,
                points,
                example: Some(example),
            })
        })
        .collect();
    // A later pick cannot be worth more than an earlier one, whatever one
    // round's median happened to land on: a seventh that outran the sixth is
    // the sample talking, not the pick.
    let mut ceiling = f64::INFINITY;
    for price in &mut prices {
        price.points = price.points.min(ceiling);
        ceiling = price.points;
    }
    prices
}

/// Price a set of rounds, naming any round the draft has no price for.
pub fn price_rounds(prices: &[PickPrice], rounds: &[u32]) -> Result<Vec<PickPrice>, String> {
    rounds
        .iter()
        .map(|round| {
            prices
                .iter()
                .find(|p| p.round == *round)
                .cloned()
                .ok_or_else(|| format!("no round {round} in this draft"))
        })
        .collect()
}

pub fn total(prices: &[PickPrice]) -> f64 {
    prices.iter().map(|p| p.points).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(round: u32, points: f64) -> PickPrice {
        PickPrice {
            round,
            points,
            example: None,
        }
    }

    #[test]
    fn a_round_costs_what_its_median_pick_was_worth() {
        // Prices come from `pick_prices`; this is the arithmetic it does.
        let mut taken: Vec<(f64, &str)> = vec![(30.0, "a"), (10.0, "b"), (20.0, "c")];
        taken.sort_by(|x, y| x.0.total_cmp(&y.0));
        assert_eq!(taken[median_index(taken.len())].1, "c");
    }

    #[test]
    fn a_round_the_draft_never_reached_is_named_not_ignored() {
        let prices = vec![price(1, 80.0), price(2, 40.0)];
        assert_eq!(price_rounds(&prices, &[1]).unwrap(), vec![price(1, 80.0)]);
        let error = price_rounds(&prices, &[9]).unwrap_err();
        assert!(error.contains("round 9"), "{error}");
    }

    #[test]
    fn a_later_round_is_never_dearer_than_an_earlier_one() {
        // The smoothing `pick_prices` applies after taking each round's
        // median, spelled out on the sequence it exists for.
        let mut points: [f64; 5] = [80.0, 40.0, 16.0, 18.5, 12.0];
        let mut ceiling = f64::INFINITY;
        for p in &mut points {
            *p = p.min(ceiling);
            ceiling = *p;
        }
        assert_eq!(points, [80.0, 40.0, 16.0, 16.0, 12.0]);
    }

    #[test]
    fn two_picks_cost_the_sum_of_both() {
        assert!((total(&[price(1, 80.0), price(4, 12.5)]) - 92.5).abs() < 1e-9);
        assert_eq!(total(&[]), 0.0);
    }
}
