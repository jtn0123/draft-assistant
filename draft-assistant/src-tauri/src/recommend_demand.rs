//! How many of a position this league actually starts, and how to say it.
//!
//! A flex slot used to be split evenly between the positions eligible for it,
//! so a SUPER_FLEX handed a quarter of itself to each of QB, RB, WR and TE.
//! That is not what a superflex league does with the slot and it is not what
//! `valuation.rs` does with it either: there the slot goes to whichever
//! eligible position values it most, which is how the replacement level a
//! candidate's VORP is measured against gets built. With the two models
//! disagreeing, a superflex roster with one quarterback read as 0.25 QBs
//! short and drafted a third running back.
//!
//! So the need model asks the replacement model. Same allocator, same board,
//! one answer.

use crate::board::AvailablePlayer;
use crate::roster::RosterRules;
use crate::valuation::{allocate_demand, ScoredPlayer};
use std::collections::HashMap;

/// Per-team starting demand by position: two RB slots plus most of a FLEX is
/// 2.7 running backs. Fractional on purpose — a FLEX is one body that several
/// positions compete for, and the early-depth term has to price a tight end's
/// claim on it against a receiver's.
pub(crate) fn starting_demand(
    available: &[AvailablePlayer],
    rules: &RosterRules,
    teams: u32,
) -> HashMap<String, f64> {
    let teams = teams.max(1) as usize;
    let pool: Vec<ScoredPlayer> = available
        .iter()
        .map(|a| ScoredPlayer {
            position: a.player.position.clone(),
            points: a.player.points,
        })
        .collect();
    allocate_demand(&pool, rules, teams, None)
        .into_iter()
        .map(|(position, count)| (position, count as f64 / teams as f64))
        .collect()
}

/// The shared slot a position's fractional demand comes out of, named the way
/// a drafter would name it.
fn shared_slot(rules: &RosterRules, position: &str) -> &'static str {
    let mut name = "flex";
    for slot in rules.slots() {
        let Some(eligible) = RosterRules::flex_eligible(slot) else {
            continue;
        };
        if eligible.contains(&position) && slot.as_str() == "SUPER_FLEX" {
            name = "superflex";
        }
    }
    name
}

/// What the league starts at a position, in words that survive the
/// arithmetic. The old phrase rounded 1.25 up and called it "about 2 with
/// flex", which is two claims the score does not make: the league does not
/// start two of them, and the term next to the phrase was worth 1.25 slots,
/// not 2.
pub(crate) fn starters_phrase(rules: &RosterRules, position: &str, demand: f64) -> String {
    let whole = demand.floor();
    let fraction = demand - whole;
    let whole = whole.max(0.0) as u32;
    let plural = |n: u32| if n == 1 { "starter" } else { "starters" };
    if fraction < 0.05 {
        return format!("{whole} {}", plural(whole));
    }
    if fraction > 0.95 {
        return format!("{} {}", whole + 1, plural(whole + 1));
    }
    let shared = shared_slot(rules, position);
    if whole == 0 {
        return format!("a share of the {shared}");
    }
    format!("{whole} {} plus a share of the {shared}", plural(whole))
}

/// How many of a position this league starts in a slot of its own — its
/// dedicated slots, plus SUPER_FLEX for a quarterback, which is what a
/// superflex league *is*. The backup discipline caps roster counts against it.
pub(crate) fn dedicated_starters(rules: &RosterRules, position: &str) -> u32 {
    rules
        .slots()
        .iter()
        .filter(|slot| {
            slot.as_str() == position || (position == "QB" && slot.as_str() == "SUPER_FLEX")
        })
        .count() as u32
}
