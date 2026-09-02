//! Shaping Sleeper's rows before the season sweep uses them: merging the
//! weeks' transaction batches, and pairing rosters into games.
//!
//! Its own file because neither touches the network, the cache or the engine —
//! they are the parts of the sweep that can be read and tested as plain
//! functions over what Sleeper returned.

use crate::season_api::{Matchup, Transaction};
use std::collections::{HashMap, HashSet};

/// Merge the weeks' transaction batches into one list, keeping the first copy
/// of each id — the same claim is reported in both weeks' responses — and
/// turning a failed week into a warning rather than losing the other one.
///
/// A set rather than a scan back over everything kept so far: that check was
/// quadratic in a list that runs to hundreds of rows.
pub(crate) fn merge_transactions(
    batches: Vec<(u32, Result<Vec<Transaction>, String>)>,
    warnings: &mut Vec<String>,
) -> Vec<Transaction> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut merged: Vec<Transaction> = Vec::new();
    for (week, result) in batches {
        match result {
            Ok(batch) => merged.extend(
                batch
                    .into_iter()
                    .filter(|t| seen.insert(t.transaction_id.clone())),
            ),
            Err(error) => {
                warnings.push(format!("transactions for week {week} unavailable: {error}"))
            }
        }
    }
    merged
}

/// Pair up the rosters sharing a matchup_id. Sleeper gives two rows per game
/// with no home/away distinction, so the lower roster id is treated as home —
/// arbitrary but stable, and the simulation is symmetric anyway.
pub(crate) fn pairs_from(matchups: &[Matchup]) -> Vec<(u32, u32)> {
    let mut by_id: HashMap<u32, Vec<u32>> = HashMap::new();
    for m in matchups {
        if let Some(id) = m.matchup_id {
            by_id.entry(id).or_default().push(m.roster_id);
        }
    }
    let mut pairs: Vec<(u32, u32)> = by_id
        .into_values()
        .filter_map(|mut rosters| {
            rosters.sort_unstable();
            match rosters.as_slice() {
                [home, away] => Some((*home, *away)),
                _ => None,
            }
        })
        .collect();
    pairs.sort_unstable();
    pairs
}

/// A matchup row as Sleeper sends it, for the tests on both sides of this
/// seam: the pairing below, and the week cache in `season_engine`.
#[cfg(test)]
pub(crate) fn matchup(roster_id: u32, matchup_id: Option<u32>) -> Matchup {
    Matchup {
        roster_id,
        matchup_id,
        points: 0.0,
        custom_points: None,
        starters: None,
        players: None,
        players_points: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction(id: &str) -> Transaction {
        Transaction {
            transaction_id: id.into(),
            kind: "waiver".into(),
            status: "complete".into(),
            created: 0,
            adds: None,
            drops: None,
            roster_ids: Vec::new(),
            draft_picks: Vec::new(),
            settings: None,
        }
    }

    #[test]
    fn a_claim_reported_in_both_weeks_is_listed_once() {
        let mut warnings = Vec::new();
        let merged = merge_transactions(
            vec![
                (4, Ok(vec![transaction("a"), transaction("b")])),
                (5, Ok(vec![transaction("b"), transaction("c")])),
            ],
            &mut warnings,
        );
        let ids: Vec<&str> = merged.iter().map(|t| t.transaction_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn one_failed_week_warns_and_keeps_the_other() {
        let mut warnings = Vec::new();
        let merged = merge_transactions(
            vec![(4, Err("503".into())), (5, Ok(vec![transaction("c")]))],
            &mut warnings,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("week 4"), "{}", warnings[0]);
    }

    #[test]
    fn matchup_ids_pair_rosters_into_games() {
        let pairs = pairs_from(&[
            matchup(3, Some(1)),
            matchup(1, Some(1)),
            matchup(2, Some(2)),
            matchup(4, Some(2)),
        ]);
        assert_eq!(pairs, vec![(1, 3), (2, 4)]);
    }

    #[test]
    fn byes_and_unscheduled_rosters_produce_no_game() {
        // A lone roster on a matchup id, and one with no id at all.
        let pairs = pairs_from(&[matchup(1, Some(1)), matchup(2, None)]);
        assert!(pairs.is_empty());
    }
}
