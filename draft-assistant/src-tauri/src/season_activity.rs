//! The league activity feed: what every team did, newest first.
//!
//! Sleeper's transaction log is one row per move with ids in it; this turns
//! each into a sentence, and carries the ids along so the feed can show the
//! faces involved rather than being a wall of text.

use crate::roster::RosterRules;
use crate::season_api::{Roster, Transaction};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct ActivityItem {
    /// "Waiver", "Trade", "Add", "Drop".
    pub kind: String,
    pub text: String,
    /// Epoch milliseconds.
    pub created: i64,
    /// The team the move belongs to, for its manager's picture.
    pub roster_id: Option<u32>,
    /// The players involved, so the row can show their faces. Adds first,
    /// then drops; capped, since a row only has space for a few.
    pub player_ids: Vec<String>,
}

/// Turn raw transactions into the league activity feed.
pub fn activity(
    transactions: &[Transaction],
    team_name: &impl Fn(u32) -> String,
    player_name: &impl Fn(&str) -> String,
    limit: usize,
) -> Vec<ActivityItem> {
    let mut items: Vec<ActivityItem> = transactions
        .iter()
        .filter(|t| t.status == "complete")
        .filter_map(|t| describe(t, team_name, player_name))
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.created));
    items.truncate(limit);
    items
}

/// Player ids out of an adds/drops map, in the same order the names come out.
fn ids_for(
    map: &Option<HashMap<String, u32>>,
    player_name: &impl Fn(&str) -> String,
) -> Vec<String> {
    let mut ids: Vec<String> = map
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    ids.sort_by_key(|id| player_name(id));
    ids
}

fn names_for(
    map: &Option<HashMap<String, u32>>,
    player_name: &impl Fn(&str) -> String,
) -> Vec<String> {
    let mut names: Vec<String> = map
        .as_ref()
        .map(|m| m.keys().map(|id| player_name(id)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn describe(
    transaction: &Transaction,
    team_name: &impl Fn(u32) -> String,
    player_name: &impl Fn(&str) -> String,
) -> Option<ActivityItem> {
    let adds = names_for(&transaction.adds, player_name);
    let drops = names_for(&transaction.drops, player_name);
    let team = transaction
        .roster_ids
        .first()
        .map(|id| team_name(*id))
        .unwrap_or_else(|| "A team".to_string());

    let (kind, text) = match transaction.kind.as_str() {
        "trade" => {
            let sides = crate::season_deals::sides_of(transaction, team_name, player_name);
            ("Trade", crate::season_deals::summary(&sides))
        }
        "waiver" => {
            if adds.is_empty() {
                return None;
            }
            let bid = transaction
                .bid()
                .filter(|b| *b > 0)
                .map(|b| format!(" for ${b}"))
                .unwrap_or_default();
            let dropped = if drops.is_empty() {
                String::new()
            } else {
                format!(", dropping {}", drops.join(", "))
            };
            (
                "Waiver",
                format!("{team} claimed {}{bid}{dropped}", adds.join(", ")),
            )
        }
        "free_agent" => match (adds.is_empty(), drops.is_empty()) {
            (true, true) => return None,
            (true, false) => ("Drop", format!("{team} dropped {}", drops.join(", "))),
            (false, true) => ("Add", format!("{team} added {}", adds.join(", "))),
            (false, false) => (
                "Add",
                format!(
                    "{team} added {} and dropped {}",
                    adds.join(", "),
                    drops.join(", ")
                ),
            ),
        },
        _ => return None,
    };
    let mut player_ids = ids_for(&transaction.adds, player_name);
    player_ids.extend(ids_for(&transaction.drops, player_name));
    player_ids.truncate(4);
    Some(ActivityItem {
        kind: kind.to_string(),
        text,
        created: transaction.created,
        roster_id: transaction.roster_ids.first().copied(),
        player_ids,
    })
}

/// One "Lineup" activity item per roster that has left a starting slot empty
/// this week. Sleeper writes "0" (or nothing) into a starter slot nobody
/// fills; starters are in the league's slot order with the bench excluded.
/// `observed_at` is epoch seconds (the feed itself is in milliseconds).
pub fn lineup_gaps(
    rosters: &[Roster],
    rules: &RosterRules,
    team_name: &impl Fn(u32) -> String,
    observed_at: u64,
) -> Vec<ActivityItem> {
    let created = observed_at as i64 * 1000;
    let slots: Vec<&str> = rules
        .slots()
        .iter()
        .map(String::as_str)
        .filter(|s| !RosterRules::is_non_starting(s))
        .collect();
    rosters
        .iter()
        .filter_map(|roster| {
            let empty: Vec<&str> = roster
                .starter_ids()
                .iter()
                .enumerate()
                .filter(|(_, id)| id.is_empty() || id.as_str() == "0")
                .map(|(i, _)| slots.get(i).copied().unwrap_or("starter"))
                .collect();
            if empty.is_empty() {
                return None;
            }
            let team = team_name(roster.roster_id);
            let text = if empty.len() == 1 {
                format!("{team} has an empty {} slot", empty[0])
            } else {
                format!(
                    "{team} has {} empty starter slots ({})",
                    empty.len(),
                    empty.join(", ")
                )
            };
            Some(ActivityItem {
                kind: "Lineup".to_string(),
                text,
                created,
                roster_id: Some(roster.roster_id),
                player_ids: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn rules(slots: &[&str]) -> RosterRules {
        RosterRules::new(&slots.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    fn transaction(
        kind: &str,
        created: i64,
        adds: &[(&str, u32)],
        drops: &[(&str, u32)],
    ) -> Transaction {
        Transaction {
            transaction_id: format!("t{created}"),
            kind: kind.into(),
            status: "complete".into(),
            created,
            adds: Some(adds.iter().map(|(p, r)| ((*p).to_string(), *r)).collect())
                .filter(|m: &HashMap<String, u32>| !m.is_empty()),
            drops: Some(drops.iter().map(|(p, r)| ((*p).to_string(), *r)).collect())
                .filter(|m: &HashMap<String, u32>| !m.is_empty()),
            roster_ids: vec![1],
            settings: Some(TransactionSettings {
                waiver_bid: Some(16),
            }),
        }
    }

    use crate::season_api::TransactionSettings;

    #[test]
    fn activity_is_newest_first_and_names_teams_and_players() {
        let team = |id: u32| format!("Team{id}");
        let player = |id: &str| id.to_uppercase();
        let items = activity(
            &[
                transaction("waiver", 100, &[("tuten", 1)], &[]),
                transaction("free_agent", 300, &[], &[("godwin", 1)]),
            ],
            &team,
            &player,
            10,
        );
        assert_eq!(items[0].kind, "Drop");
        assert_eq!(items[0].text, "Team1 dropped GODWIN");
        assert_eq!(items[1].kind, "Waiver");
        assert_eq!(items[1].text, "Team1 claimed TUTEN for $16");
    }

    #[test]
    fn incomplete_transactions_never_reach_the_feed() {
        let mut failed = transaction("waiver", 100, &[("x", 1)], &[]);
        failed.status = "failed".into();
        let items = activity(&[failed], &|id| format!("T{id}"), &|id| id.into(), 10);
        assert!(items.is_empty());
    }

    fn roster(roster_id: u32, starters: &[&str]) -> Roster {
        Roster {
            roster_id,
            owner_id: None,
            players: None,
            starters: Some(starters.iter().map(|s| (*s).to_string()).collect()),
            reserve: None,
            settings: Default::default(),
        }
    }

    #[test]
    fn lineup_gaps_name_the_empty_slot_per_team() {
        let rules = rules(&["QB", "RB", "WR", "FLEX", "BN", "BN"]);
        let items = lineup_gaps(
            &[
                roster(1, &["1", "2", "3", "4"]),
                roster(2, &["1", "0", "3", "4"]),
                roster(3, &["1", "2", "", "0"]),
            ],
            &rules,
            &|id| format!("Team{id}"),
            99,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "Lineup");
        assert_eq!(items[0].text, "Team2 has an empty RB slot");
        assert_eq!(items[1].text, "Team3 has 2 empty starter slots (WR, FLEX)");
        assert_eq!(items[1].created, 99_000);
    }
}
