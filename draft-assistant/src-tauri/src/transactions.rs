//! What the league has been doing: `/league/{id}/transactions/{week}` read
//! as a timeline with names on it. What the user could do about it lives in
//! `trade.rs`.

use crate::draft::TeamRoster;
use crate::loaded::LoadedLeague;
use crate::sleeper::Transaction;
use serde::Serialize;
use std::collections::HashMap;

/// Timeline entries kept, newest first.
const ACTIVITY_SHOWN: usize = 40;

#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    /// ms since the epoch.
    pub at: u64,
    pub week: u32,
    /// "trade" | "waiver" | "free_agent" | "commissioner".
    pub kind: String,
    /// "complete", or "failed" for a waiver claim that lost — kept because
    /// what a rival bid on, and how much, says what they value.
    pub status: String,
    pub teams: Vec<String>,
    /// (team, player) pairs.
    pub adds: Vec<(String, String)>,
    pub drops: Vec<(String, String)>,
    /// Draft picks that moved, described.
    pub picks: Vec<String>,
    pub bid: Option<u32>,
}

pub fn timeline(
    transactions: &[(u32, Vec<Transaction>)],
    team_of_roster: &dyn Fn(u32) -> Option<String>,
    name_of: &dyn Fn(&str) -> String,
) -> Vec<Activity> {
    let team = |r: u32| team_of_roster(r).unwrap_or_else(|| format!("roster {r}"));
    let mut out: Vec<Activity> = transactions
        .iter()
        .flat_map(|(week, list)| list.iter().map(move |t| (*week, t)))
        .filter(|(_, t)| t.status == "complete" || (t.kind == "waiver" && t.status == "failed"))
        .map(|(week, t)| Activity {
            at: t.created,
            week,
            kind: t.kind.clone(),
            status: t.status.clone(),
            teams: t.roster_ids.iter().map(|r| team(*r)).collect(),
            adds: t
                .adds
                .iter()
                .flatten()
                .map(|(pid, r)| (team(*r), name_of(pid)))
                .collect(),
            drops: t
                .drops
                .iter()
                .flatten()
                .map(|(pid, r)| (team(*r), name_of(pid)))
                .collect(),
            picks: t
                .draft_picks
                .iter()
                .map(|p| {
                    format!(
                        "{} round {} ({}) → {}",
                        p.season,
                        p.round,
                        team(p.roster_id),
                        team(p.owner_id)
                    )
                })
                .collect(),
            bid: t.settings.as_ref().and_then(|s| s.waiver_bid),
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out.truncate(ACTIVITY_SHOWN);
    out
}

/// Names for the feed: the board first, then the player dictionary, then
/// the id itself. Dropped players are often off the board.
pub fn name_lookup(loaded: &LoadedLeague) -> impl Fn(&str) -> String + '_ {
    move |id: &str| {
        loaded
            .board_index
            .get(id)
            .map(|&i| loaded.board[i].name.clone())
            .or_else(|| loaded.player_meta.get(id).and_then(|m| m.full_name.clone()))
            .unwrap_or_else(|| id.to_string())
    }
}

/// Roster id -> the display name of the slot that roster drafted from.
pub fn team_lookup<'a>(
    loaded: &'a LoadedLeague,
    rosters: &'a [TeamRoster],
) -> impl Fn(u32) -> Option<String> + 'a {
    let slot_of: HashMap<u32, u32> = loaded
        .draft
        .slot_to_roster_id
        .as_ref()
        .map(|m| {
            m.iter()
                .filter_map(|(s, r)| s.parse().ok().map(|s: u32| (*r, s)))
                .collect()
        })
        .unwrap_or_default();
    move |roster_id: u32| {
        let slot = *slot_of.get(&roster_id)?;
        rosters
            .get((slot - 1) as usize)
            .and_then(|r| r.display_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sleeper::{TradedPick, TransactionSettings};

    fn tx(kind: &str, created: u64, rosters: &[u32]) -> Transaction {
        Transaction {
            transaction_id: format!("{created}"),
            kind: kind.into(),
            status: "complete".into(),
            created,
            roster_ids: rosters.to_vec(),
            adds: None,
            drops: None,
            draft_picks: Vec::new(),
            settings: None,
        }
    }

    #[test]
    fn the_feed_is_newest_first_with_names_and_bids() {
        let mut waiver = tx("waiver", 200, &[5]);
        waiver.adds = Some(HashMap::from([("p1".to_string(), 5)]));
        waiver.drops = Some(HashMap::from([("p2".to_string(), 5)]));
        waiver.settings = Some(TransactionSettings {
            waiver_bid: Some(37),
        });
        let mut trade = tx("trade", 100, &[2, 4]);
        trade.draft_picks = vec![TradedPick {
            season: "2026".into(),
            round: 3,
            roster_id: 2,
            owner_id: 4,
            previous_owner_id: Some(2),
        }];
        let mut failed = tx("waiver", 300, &[5]);
        failed.status = "failed".into();
        let mut failed_add = tx("free_agent", 400, &[6]);
        failed_add.status = "failed".into();
        let list = vec![(1, vec![trade, waiver, failed, failed_add])];
        let team = |r: u32| Some(format!("T{r}"));
        let name = |id: &str| format!("Player {id}");
        let feed = timeline(&list, &team, &name);
        // A lost claim is kept (it says what a rival bid on); a failed add is noise.
        assert_eq!(feed.len(), 3, "{feed:?}");
        assert_eq!(
            (feed[0].kind.as_str(), feed[0].status.as_str()),
            ("waiver", "failed")
        );
        assert_eq!(feed[1].kind, "waiver");
        assert_eq!(feed[1].status, "complete");
        assert_eq!(
            feed[1].adds,
            vec![("T5".to_string(), "Player p1".to_string())]
        );
        assert_eq!(feed[1].bid, Some(37));
        assert_eq!(feed[2].picks, vec!["2026 round 3 (T2) → T4"]);
        assert_eq!(feed[2].teams, vec!["T2", "T4"]);
    }
}
