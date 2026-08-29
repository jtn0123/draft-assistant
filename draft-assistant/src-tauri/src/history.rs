//! Last season, read for what it says about the people: who trades, who
//! churns the wire, who spends their FAAB, and what a winning claim cost.
//! Built once from the previous league's rosters, users and transactions
//! and cached for a week — it does not change.

use crate::sleeper::{LeagueRoster, LeagueUser, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BidStats {
    /// Winning claims that carried a bid.
    pub count: u32,
    pub median: u32,
    pub p75: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    pub trades: u32,
    /// Waiver claims and free-agent adds, together.
    pub moves: u32,
    pub faab_used: u32,
    pub wins: u32,
    pub losses: u32,
    pub points_for: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueHistory {
    pub league_id: String,
    pub trades: u32,
    pub claims: u32,
    pub bids: BidStats,
    /// Most active traders first.
    pub managers: Vec<ManagerProfile>,
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn build(
    league_id: &str,
    rosters: &[LeagueRoster],
    users: &[LeagueUser],
    transactions: &[(u32, Vec<Transaction>)],
) -> LeagueHistory {
    let names: HashMap<&str, Option<String>> = users
        .iter()
        .map(|u| (u.user_id.as_str(), u.display_name.clone()))
        .collect();
    let owner_of: HashMap<u32, &str> = rosters
        .iter()
        .filter_map(|r| Some((r.roster_id, r.owner_id.as_deref()?)))
        .collect();
    let mut trades_by: HashMap<&str, u32> = HashMap::new();
    let mut moves_by: HashMap<&str, u32> = HashMap::new();
    let mut bids: Vec<u32> = Vec::new();
    let mut trades = 0;
    let mut claims = 0;
    for (_, list) in transactions {
        for t in list.iter().filter(|t| t.status == "complete") {
            match t.kind.as_str() {
                "trade" => {
                    trades += 1;
                    for r in &t.roster_ids {
                        if let Some(o) = owner_of.get(r) {
                            *trades_by.entry(o).or_insert(0) += 1;
                        }
                    }
                }
                "waiver" | "free_agent" => {
                    if t.kind == "waiver" {
                        claims += 1;
                        if let Some(b) = t.settings.as_ref().and_then(|s| s.waiver_bid) {
                            if b > 0 {
                                bids.push(b);
                            }
                        }
                    }
                    for r in &t.roster_ids {
                        if let Some(o) = owner_of.get(r) {
                            *moves_by.entry(o).or_insert(0) += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    bids.sort_unstable();
    let mut managers: Vec<ManagerProfile> = rosters
        .iter()
        .filter_map(|r| {
            let uid = r.owner_id.as_deref()?;
            Some(ManagerProfile {
                user_id: uid.to_string(),
                display_name: names.get(uid).cloned().flatten(),
                trades: trades_by.get(uid).copied().unwrap_or(0),
                moves: moves_by.get(uid).copied().unwrap_or(0),
                faab_used: r.settings.waiver_budget_used,
                wins: r.settings.wins,
                losses: r.settings.losses,
                points_for: f64::from(r.settings.fpts) + f64::from(r.settings.fpts_decimal) / 100.0,
            })
        })
        .collect();
    managers.sort_by(|a, b| b.trades.cmp(&a.trades).then(b.moves.cmp(&a.moves)));
    LeagueHistory {
        league_id: league_id.to_string(),
        trades,
        claims,
        bids: BidStats {
            count: bids.len() as u32,
            median: percentile(&bids, 0.5),
            p75: percentile(&bids, 0.75),
            max: bids.last().copied().unwrap_or(0),
        },
        managers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sleeper::{RosterSettings, TransactionSettings};

    fn tx(kind: &str, rosters: &[u32], bid: Option<u32>) -> Transaction {
        Transaction {
            transaction_id: "t".into(),
            kind: kind.into(),
            status: "complete".into(),
            created: 0,
            roster_ids: rosters.to_vec(),
            adds: None,
            drops: None,
            draft_picks: Vec::new(),
            settings: bid.map(|waiver_bid| TransactionSettings {
                waiver_bid: Some(waiver_bid),
            }),
        }
    }

    fn roster(id: u32, owner: &str, faab: u32) -> LeagueRoster {
        LeagueRoster {
            roster_id: id,
            owner_id: Some(owner.into()),
            settings: RosterSettings {
                wins: 10,
                losses: 4,
                waiver_budget_used: faab,
                ..Default::default()
            },
            starters: Vec::new(),
            players: Vec::new(),
        }
    }

    #[test]
    fn counts_trades_moves_and_bids_per_manager() {
        let rosters = [roster(1, "a", 1000), roster(2, "b", 140)];
        let users = [LeagueUser {
            user_id: "a".into(),
            display_name: Some("Alice".into()),
        }];
        let txs = vec![(
            1,
            vec![
                tx("trade", &[1, 2], None),
                tx("waiver", &[1], Some(50)),
                tx("waiver", &[1], Some(10)),
                tx("waiver", &[2], Some(200)),
                tx("free_agent", &[2], None),
                tx("waiver", &[2], Some(0)),
            ],
        )];
        let h = build("prev", &rosters, &users, &txs);
        assert_eq!((h.trades, h.claims), (1, 4));
        assert_eq!(
            (h.bids.count, h.bids.median, h.bids.p75, h.bids.max),
            (3, 50, 200, 200)
        );
        // Equal on trades, so the busier manager on the wire comes first.
        assert_eq!(h.managers[0].user_id, "b", "{:?}", h.managers);
        let b = &h.managers[0];
        assert_eq!((b.trades, b.moves, b.display_name.is_none()), (1, 3, true));
        let a = &h.managers[1];
        assert_eq!((a.trades, a.moves, a.faab_used), (1, 2, 1000));
        assert_eq!(a.display_name.as_deref(), Some("Alice"));
    }
}
