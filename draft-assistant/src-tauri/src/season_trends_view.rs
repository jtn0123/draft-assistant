//! The Trends tab's data: strength series per team and the explained change
//! feed, built from `season_history` snapshots and the league's transactions.

use crate::season_api::Transaction;
use crate::season_history::{History, TeamSnap};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Strength moves smaller than this are noise, not news.
const MIN_REPORTED_DELTA: f64 = 0.3;
/// Per-player projection moves smaller than this are not worth a line.
const MIN_PLAYER_DELTA: f64 = 0.8;

#[derive(Debug, Clone, Serialize)]
pub struct TrendPoint {
    /// Seconds since epoch.
    pub at: u64,
    pub week: u32,
    pub strength: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamSeries {
    pub roster_id: u32,
    pub name: String,
    pub is_mine: bool,
    pub points: Vec<TrendPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrendChange {
    pub at: u64,
    pub week: u32,
    pub roster_id: u32,
    pub team: String,
    pub is_mine: bool,
    /// Change in strength, points per week.
    pub delta: f64,
    /// Up to three explanations, biggest first.
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TrendsView {
    pub series: Vec<TeamSeries>,
    pub changes: Vec<TrendChange>,
}

/// The transaction that moved `player` onto or off `roster` in a window.
fn move_for<'a>(
    transactions: &'a [Transaction],
    player: &str,
    roster: u32,
    from_secs: u64,
    to_secs: u64,
    added: bool,
) -> Option<&'a Transaction> {
    // A snapshot is taken after the load that saw the move, so allow the
    // transaction to predate the previous snapshot by a little.
    let from_ms = (from_secs.saturating_sub(3600) * 1000) as i64;
    let to_ms = (to_secs * 1000) as i64;
    transactions
        .iter()
        .filter(|t| t.status == "complete" && t.created >= from_ms && t.created <= to_ms)
        .find(|t| {
            let side = if added { &t.adds } else { &t.drops };
            side.as_ref()
                .is_some_and(|m| m.get(player).copied() == Some(roster))
        })
}

fn other_side(transaction: &Transaction, roster: u32) -> Vec<&str> {
    // What this roster gave up in the same deal.
    transaction
        .drops
        .as_ref()
        .map(|m| {
            m.iter()
                .filter(|(_, r)| **r == roster)
                .map(|(id, _)| id.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// One team's step between two snapshots, explained.
fn explain(
    prev: &TeamSnap,
    next: &TeamSnap,
    window: (u64, u64),
    transactions: &[Transaction],
    player_name: &impl Fn(&str) -> String,
) -> Vec<String> {
    // (impact, text) so the biggest movers lead.
    let mut reasons: Vec<(f64, String)> = Vec::new();
    let mut explained_drops: HashSet<&str> = HashSet::new();

    for (id, snap) in &next.players {
        if prev.players.contains_key(id) {
            continue;
        }
        let name = player_name(id);
        let text = match move_for(transactions, id, next.roster_id, window.0, window.1, true) {
            Some(t) if t.kind == "trade" => {
                let gave: Vec<String> = other_side(t, next.roster_id)
                    .into_iter()
                    .map(|g| {
                        explained_drops.insert(g);
                        player_name(g)
                    })
                    .collect();
                if gave.is_empty() {
                    format!("traded for {name}")
                } else {
                    format!("traded {} for {name}", gave.join(" + "))
                }
            }
            Some(t) if t.kind == "waiver" => match t.settings.as_ref().and_then(|s| s.waiver_bid) {
                Some(bid) => format!("claimed {name} for ${bid}"),
                None => format!("claimed {name}"),
            },
            _ => format!("added {name}"),
        };
        reasons.push((snap.points, text));
    }

    for (id, snap) in &prev.players {
        if next.players.contains_key(id) || explained_drops.contains(id.as_str()) {
            continue;
        }
        reasons.push((-snap.points, format!("dropped {}", player_name(id))));
    }

    for (id, now) in &next.players {
        let Some(before) = prev.players.get(id) else {
            continue;
        };
        let delta = now.points - before.points;
        if now.injury != before.injury {
            let tag = now.injury.as_deref().unwrap_or("healthy");
            reasons.push((
                delta,
                format!("{} now {tag} ({:+.1}/wk)", player_name(id), delta),
            ));
        } else if delta.abs() >= MIN_PLAYER_DELTA {
            reasons.push((
                delta,
                format!("{} projection {delta:+.1}/wk", player_name(id)),
            ));
        }
    }

    reasons.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    reasons.into_iter().take(3).map(|(_, text)| text).collect()
}

/// Build the graph series and the change feed.
pub fn trends_view(
    history: &History,
    transactions: &[Transaction],
    name_of: &impl Fn(u32) -> String,
    player_name: &impl Fn(&str) -> String,
    my_roster_id: Option<u32>,
    limit: usize,
) -> TrendsView {
    let mut by_team: HashMap<u32, Vec<TrendPoint>> = HashMap::new();
    for snapshot in &history.snapshots {
        for team in &snapshot.teams {
            by_team.entry(team.roster_id).or_default().push(TrendPoint {
                at: snapshot.taken_at,
                week: snapshot.week,
                strength: team.strength,
            });
        }
    }
    let mut series: Vec<TeamSeries> = by_team
        .into_iter()
        .map(|(roster_id, points)| TeamSeries {
            roster_id,
            name: name_of(roster_id),
            is_mine: Some(roster_id) == my_roster_id,
            points,
        })
        .collect();
    // Strongest today first; the legend reads top to bottom.
    series.sort_by(|a, b| {
        let last = |s: &TeamSeries| s.points.last().map_or(0.0, |p| p.strength);
        last(b).total_cmp(&last(a))
    });

    let mut changes = Vec::new();
    for pair in history.snapshots.windows(2) {
        let (prev, next) = (&pair[0], &pair[1]);
        for team in &next.teams {
            let Some(before) = prev.teams.iter().find(|t| t.roster_id == team.roster_id) else {
                continue;
            };
            let delta = team.strength - before.strength;
            let reasons = explain(
                before,
                team,
                (prev.taken_at, next.taken_at),
                transactions,
                player_name,
            );
            if delta.abs() < MIN_REPORTED_DELTA && reasons.is_empty() {
                continue;
            }
            changes.push(TrendChange {
                at: next.taken_at,
                week: next.week,
                roster_id: team.roster_id,
                team: name_of(team.roster_id),
                is_mine: Some(team.roster_id) == my_roster_id,
                delta,
                reasons,
            });
        }
    }
    changes.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| b.delta.abs().total_cmp(&a.delta.abs()))
    });
    changes.truncate(limit);

    TrendsView { series, changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::season_history::{push, PlayerSnap, Snapshot};

    fn team(roster_id: u32, strength: f64, players: &[(&str, f64)]) -> TeamSnap {
        TeamSnap {
            roster_id,
            strength,
            players: players
                .iter()
                .map(|(id, points)| {
                    (
                        (*id).to_string(),
                        PlayerSnap {
                            points: *points,
                            injury: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn snapshot(at: u64, teams: Vec<TeamSnap>) -> Snapshot {
        Snapshot {
            taken_at: at,
            week: 3,
            teams,
        }
    }

    fn trade(created_secs: u64, adds: &[(&str, u32)], drops: &[(&str, u32)]) -> Transaction {
        Transaction {
            transaction_id: format!("t{created_secs}"),
            kind: "trade".into(),
            status: "complete".into(),
            created: (created_secs * 1000) as i64,
            adds: Some(adds.iter().map(|(p, r)| ((*p).to_string(), *r)).collect()),
            drops: Some(drops.iter().map(|(p, r)| ((*p).to_string(), *r)).collect()),
            roster_ids: vec![1, 2],
            draft_picks: Vec::new(),
            settings: Default::default(),
        }
    }

    fn name(id: &str) -> String {
        id.to_uppercase()
    }

    #[test]
    fn a_trade_is_named_on_both_sides() {
        let mut history = History::default();
        push(
            &mut history,
            snapshot(
                10_000,
                vec![
                    team(1, 100.0, &[("cd", 18.0)]),
                    team(2, 90.0, &[("te", 12.0)]),
                ],
            ),
        );
        push(
            &mut history,
            snapshot(
                20_000,
                vec![
                    team(1, 95.0, &[("te", 12.0)]),
                    team(2, 96.0, &[("cd", 18.0)]),
                ],
            ),
        );
        let deal = trade(15_000, &[("te", 1), ("cd", 2)], &[("cd", 1), ("te", 2)]);
        let view = trends_view(&history, &[deal], &|r| format!("T{r}"), &name, Some(2), 10);
        assert_eq!(view.series[0].roster_id, 2, "strongest today leads");
        assert!(view.series[0].is_mine);
        let mine = view.changes.iter().find(|c| c.roster_id == 2).unwrap();
        assert!((mine.delta - 6.0).abs() < 1e-9);
        assert_eq!(mine.reasons, vec!["traded TE for CD"]);
        let theirs = view.changes.iter().find(|c| c.roster_id == 1).unwrap();
        assert_eq!(theirs.reasons, vec!["traded CD for TE"]);
    }

    #[test]
    fn projection_moves_and_injuries_are_explained_without_a_transaction() {
        let mut history = History::default();
        push(
            &mut history,
            snapshot(10_000, vec![team(1, 100.0, &[("qb", 20.0), ("rb", 12.0)])]),
        );
        let mut hurt = team(1, 92.0, &[("qb", 20.5), ("rb", 4.0)]);
        hurt.players.get_mut("rb").unwrap().injury = Some("Out".into());
        push(&mut history, snapshot(20_000, vec![hurt]));
        let view = trends_view(&history, &[], &|r| format!("T{r}"), &name, None, 10);
        assert_eq!(view.changes.len(), 1);
        assert_eq!(view.changes[0].reasons, vec!["RB now Out (-8.0/wk)"]);
    }

    #[test]
    fn small_drift_with_nothing_to_say_is_not_reported() {
        let mut history = History::default();
        push(
            &mut history,
            snapshot(10_000, vec![team(1, 100.0, &[("qb", 20.0)])]),
        );
        push(
            &mut history,
            snapshot(20_000, vec![team(1, 100.1, &[("qb", 20.1)])]),
        );
        let view = trends_view(&history, &[], &|r| format!("T{r}"), &name, None, 10);
        assert!(view.changes.is_empty());
        assert_eq!(view.series[0].points.len(), 2);
    }
}
