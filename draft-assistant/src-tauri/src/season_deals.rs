//! Trades that actually happened, one entry per deal with both sides named.
//!
//! An offer that has only been proposed stays private to the two managers, so
//! it never reaches us. A trade that both sides accepted but that is still
//! inside the league's review window does carry a status of its own, so it is
//! listed here as pending rather than dropped — everything else is a deal that
//! went through this week or last.

use crate::season_api::Transaction;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TradeSide {
    pub roster_id: u32,
    pub team: String,
    /// Player names this side received.
    pub gets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeDone {
    pub transaction_id: String,
    /// Epoch milliseconds.
    pub at: i64,
    pub sides: Vec<TradeSide>,
    pub involves_me: bool,
    /// Accepted but not yet processed — still inside the review window.
    pub pending: bool,
}

/// What each roster in a trade received, in roster_ids order.
pub fn sides_of(
    transaction: &Transaction,
    team_name: &impl Fn(u32) -> String,
    player_name: &impl Fn(&str) -> String,
) -> Vec<TradeSide> {
    transaction
        .roster_ids
        .iter()
        .map(|roster_id| {
            let mut gets: Vec<String> = transaction
                .adds
                .as_ref()
                .map(|m| {
                    m.iter()
                        .filter(|(_, r)| *r == roster_id)
                        .map(|(id, _)| player_name(id))
                        .collect()
                })
                .unwrap_or_default();
            gets.sort();
            TradeSide {
                roster_id: *roster_id,
                team: team_name(*roster_id),
                gets,
            }
        })
        .collect()
}

/// "A gets X, Y · B gets Z" — the one-line form for the activity feed.
pub fn summary(sides: &[TradeSide]) -> String {
    sides
        .iter()
        .map(|s| {
            if s.gets.is_empty() {
                format!("{} gets draft picks", s.team)
            } else {
                format!("{} gets {}", s.team, s.gets.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Trades that went through, plus any still in review, newest first.
/// A trade the league voted down (`failed`) is left out.
pub fn recent_trades(
    transactions: &[Transaction],
    team_name: &impl Fn(u32) -> String,
    player_name: &impl Fn(&str) -> String,
    my_roster_id: Option<u32>,
) -> Vec<TradeDone> {
    let mut deals: Vec<TradeDone> = transactions
        .iter()
        .filter(|t| t.kind == "trade" && t.status != "failed")
        .map(|t| TradeDone {
            transaction_id: t.transaction_id.clone(),
            at: t.created,
            sides: sides_of(t, team_name, player_name),
            involves_me: my_roster_id.is_some_and(|me| t.roster_ids.contains(&me)),
            pending: t.status != "complete",
        })
        .collect();
    deals.sort_by_key(|d| std::cmp::Reverse(d.at));
    deals
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn trade(id: &str, created: i64, adds: &[(&str, u32)]) -> Transaction {
        Transaction {
            transaction_id: id.into(),
            kind: "trade".into(),
            status: "complete".into(),
            created,
            adds: Some(adds.iter().map(|(p, r)| ((*p).to_string(), *r)).collect()),
            drops: Some(HashMap::new()),
            roster_ids: vec![11, 13],
            settings: None,
        }
    }

    fn team(id: u32) -> String {
        format!("T{id}")
    }

    fn player(id: &str) -> String {
        id.to_uppercase()
    }

    #[test]
    fn each_side_lists_what_it_received() {
        let sides = sides_of(&trade("a", 5, &[("cd", 11), ("phi", 13)]), &team, &player);
        assert_eq!(summary(&sides), "T11 gets CD · T13 gets PHI");
    }

    #[test]
    fn newest_first_and_flags_my_deals() {
        let deals = recent_trades(
            &[trade("old", 1, &[("x", 11)]), trade("new", 9, &[("y", 13)])],
            &team,
            &player,
            Some(13),
        );
        assert_eq!(deals[0].transaction_id, "new");
        assert!(deals[0].involves_me);
        assert!(deals[1].involves_me);
    }

    #[test]
    fn a_trade_still_in_review_is_listed_as_pending() {
        let mut waiting = trade("w", 5, &[("cd", 11)]);
        waiting.status = "pending".into();
        let mut voted_down = trade("v", 6, &[("x", 11)]);
        voted_down.status = "failed".into();
        let deals = recent_trades(&[waiting, voted_down], &team, &player, None);
        assert_eq!(deals.len(), 1, "a failed trade never happened");
        assert!(deals[0].pending);
    }

    #[test]
    fn a_pick_only_side_says_so() {
        let sides = sides_of(&trade("a", 5, &[("cd", 11)]), &team, &player);
        assert_eq!(summary(&sides), "T11 gets CD · T13 gets draft picks");
    }
}
