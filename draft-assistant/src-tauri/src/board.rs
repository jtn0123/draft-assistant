//! The scored draft board: players valued under the league's exact rules.

use crate::roster::RosterRules;
use crate::scoring;
use crate::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use crate::valuation::{self, ReplacementModel, ScoredPlayer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const WEEKS: u32 = 18;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardPlayer {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub bye_week: Option<u32>,
    /// Season points under THIS league's exact scoring rules.
    pub points: f64,
    /// Of which, expected per-game yardage bonus points.
    pub bonus_points: f64,
    pub vorp: f64,
    pub tier: u32,
    pub position_rank: u32,
    pub overall_rank: u32,
    pub adp: Option<f64>,
    pub injury_status: Option<String>,
    /// Sleeper's own PPR total, kept as an auditable cross-check only.
    pub sleeper_pts_ppr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailablePlayer {
    #[serde(flatten)]
    pub player: BoardPlayer,
    /// P(still available at my next pick after the current one).
    pub survival_next: Option<f64>,
}

/// Which Sleeper ADP column matches this league's market: two-QB leagues draft
/// off `adp_2qb`, otherwise the reception value picks PPR / half / standard.
/// Falls back to `adp_ppr` when a row lacks the chosen column.
pub fn adp_key(scoring: &HashMap<String, f64>, rules: &RosterRules) -> &'static str {
    let qb_slots = rules
        .slots()
        .iter()
        .filter(|slot| slot.as_str() == "QB" || slot.as_str() == "SUPER_FLEX")
        .count();
    if qb_slots >= 2 {
        return "adp_2qb";
    }
    let rec = scoring.get("rec").copied().unwrap_or(0.0);
    if rec >= 0.75 {
        "adp_ppr"
    } else if rec > 0.0 {
        "adp_half_ppr"
    } else {
        "adp_std"
    }
}

pub struct BoardBuild {
    pub players: Vec<BoardPlayer>,
    pub replacement: ReplacementModel,
}

pub fn build_board(
    league: &League,
    _draft: &Draft,
    player_meta: &HashMap<String, PlayerMeta>,
    season_rows: &[ProjectionRow],
    weekly_rows: &[ProjectionRow],
    rules: &RosterRules,
    warnings: &mut Vec<String>,
) -> BoardBuild {
    let scoring_map = &league.scoring_settings;
    let adp_column = adp_key(scoring_map, rules);

    // Positions this league actually rosters (K excluded automatically for
    // this league because there is no K slot).
    let wanted = rules.draftable_positions();

    // Weekly rows grouped per player for bonus expectations, and per-team
    // week coverage for bye inference.
    let mut weekly_by_player: HashMap<&str, Vec<&HashMap<String, f64>>> = HashMap::new();
    // (team, week) -> count of rows with a real opponent. A bye-week row
    // exists but carries no opponent, so the bye is the week with (near) zero
    // opponent rows — counted, not first-missing, because one stale row for a
    // traded player would otherwise poison the whole team.
    let mut team_week_counts: HashMap<String, [u32; WEEKS as usize]> = HashMap::new();
    for row in weekly_rows {
        if let Some(stats) = &row.stats {
            weekly_by_player
                .entry(row.player_id.as_str())
                .or_default()
                .push(stats);
        }
        if let (Some(meta), Some(week), Some(_)) = (&row.player, row.week, row.opponent.as_ref()) {
            if let Some(team) = &meta.team {
                if (1..=WEEKS).contains(&week) {
                    team_week_counts
                        .entry(team.clone())
                        .or_insert([0; WEEKS as usize])[(week - 1) as usize] += 1;
                }
            }
        }
    }
    let bye_of = |team: &Option<String>| -> Option<u32> {
        let team = team.as_ref()?;
        let counts = team_week_counts.get(team)?;
        let max = *counts.iter().max()?;
        if max == 0 {
            return None;
        }
        // The bye week has at most a stray row or two vs a full slate.
        let (week_idx, &min) = counts.iter().enumerate().min_by_key(|(_, &c)| c)?;
        if min * 4 <= max {
            Some(week_idx as u32 + 1)
        } else {
            None
        }
    };

    let mut scored: Vec<BoardPlayer> = Vec::new();
    for row in season_rows {
        let Some(stats) = &row.stats else { continue };
        let meta = row.player.as_ref();
        let position = meta
            .and_then(|m| m.position.clone())
            .or_else(|| {
                player_meta
                    .get(&row.player_id)
                    .and_then(|m| m.position.clone())
            })
            .unwrap_or_default();
        if !wanted.contains(&position) {
            continue;
        }
        let name = match position.as_str() {
            "DEF" => {
                let first = meta.and_then(|m| m.first_name.clone()).unwrap_or_default();
                let last = meta.and_then(|m| m.last_name.clone()).unwrap_or_default();
                format!("{first} {last}").trim().to_string()
            }
            _ => player_meta
                .get(&row.player_id)
                .and_then(|m| m.full_name.clone())
                .or_else(|| {
                    let first = meta.and_then(|m| m.first_name.clone()).unwrap_or_default();
                    let last = meta.and_then(|m| m.last_name.clone()).unwrap_or_default();
                    let joined = format!("{first} {last}").trim().to_string();
                    if joined.is_empty() {
                        None
                    } else {
                        Some(joined)
                    }
                })
                .unwrap_or_else(|| row.player_id.clone()),
        };
        let base = scoring::base_points(stats, scoring_map);
        let bonus = weekly_by_player
            .get(row.player_id.as_str())
            .map(|weeks| scoring::bonus_points(weeks, scoring_map))
            .unwrap_or(0.0);
        let points = base + bonus;
        if points < 20.0 {
            continue; // junk rows
        }
        let team = meta
            .and_then(|m| m.team.clone())
            .or_else(|| player_meta.get(&row.player_id).and_then(|m| m.team.clone()));
        scored.push(BoardPlayer {
            player_id: row.player_id.clone(),
            name,
            position: position.clone(),
            bye_week: bye_of(&team),
            team,
            points,
            bonus_points: bonus,
            vorp: 0.0,
            tier: 0,
            position_rank: 0,
            overall_rank: 0,
            adp: row
                .stat(adp_column)
                .or_else(|| row.stat("adp_ppr"))
                .filter(|&a| a > 0.0 && a < 500.0),
            injury_status: player_meta
                .get(&row.player_id)
                .and_then(|m| m.injury_status.clone()),
            sleeper_pts_ppr: row.stat("pts_ppr"),
        });
    }

    if scored.is_empty() {
        warnings.push("no scored players — projections fetch likely failed".into());
        return BoardBuild {
            players: scored,
            replacement: ReplacementModel::default(),
        };
    }

    // Replacement + VORP.
    let as_scored: Vec<ScoredPlayer> = scored
        .iter()
        .map(|p| ScoredPlayer {
            position: p.position.clone(),
            points: p.points,
        })
        .collect();
    let model = valuation::compute_replacement(&as_scored, rules, league.total_rosters as usize);
    for p in &mut scored {
        p.vorp = p.points - model.baseline.get(&p.position).copied().unwrap_or(0.0);
    }

    // Position ranks + tiers.
    let mut by_pos: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in scored.iter().enumerate() {
        by_pos.entry(p.position.clone()).or_default().push(i);
    }
    for (pos, idxs) in &mut by_pos {
        idxs.sort_by(|&a, &b| {
            scored[b]
                .points
                .partial_cmp(&scored[a].points)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let pts: Vec<f64> = idxs.iter().map(|&i| scored[i].points).collect();
        let tiers = valuation::assign_tiers(&pts, valuation::tier_gap_threshold(pos));
        for (rank, (&i, tier)) in idxs.iter().zip(tiers).enumerate() {
            scored[i].position_rank = rank as u32 + 1;
            scored[i].tier = tier;
        }
    }

    // Overall rank by VORP.
    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_by(|&a, &b| {
        scored[b]
            .vorp
            .partial_cmp(&scored[a].vorp)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (rank, &i) in order.iter().enumerate() {
        scored[i].overall_rank = rank as u32 + 1;
    }
    scored.sort_by(|a, b| a.overall_rank.cmp(&b.overall_rank));
    BoardBuild {
        players: scored,
        replacement: model,
    }
}

#[cfg(test)]
mod adp_tests {
    use super::*;

    fn rules(slots: &[&str]) -> RosterRules {
        RosterRules::new(&slots.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn scoring(rec: f64) -> HashMap<String, f64> {
        HashMap::from([("rec".to_string(), rec), ("pass_td".to_string(), 4.0)])
    }

    #[test]
    fn the_adp_column_follows_the_league_scoring() {
        let std = rules(&["QB", "RB", "WR", "FLEX", "BN"]);
        assert_eq!(adp_key(&scoring(1.0), &std), "adp_ppr");
        assert_eq!(adp_key(&scoring(0.5), &std), "adp_half_ppr");
        assert_eq!(adp_key(&scoring(0.0), &std), "adp_std");
        assert_eq!(adp_key(&HashMap::new(), &std), "adp_std");
    }

    #[test]
    fn two_quarterback_leagues_use_the_2qb_market() {
        assert_eq!(
            adp_key(&scoring(1.0), &rules(&["QB", "SUPER_FLEX", "BN"])),
            "adp_2qb"
        );
        assert_eq!(
            adp_key(&scoring(0.5), &rules(&["QB", "QB", "RB", "BN"])),
            "adp_2qb"
        );
    }

    #[test]
    fn a_half_ppr_board_reads_half_ppr_adp_and_falls_back_to_ppr() {
        let league: League = serde_json::from_value(serde_json::json!({
            "league_id": "l", "name": "Half", "season": "2026", "status": "pre_draft",
            "total_rosters": 2, "roster_positions": ["WR", "BN"],
            "scoring_settings": {"rec": 0.5, "rec_yd": 0.1}
        }))
        .unwrap();
        let draft: Draft = serde_json::from_value(serde_json::json!({
            "draft_id": "d", "status": "pre_draft", "type": "snake",
            "settings": {"teams": 2, "rounds": 2}
        }))
        .unwrap();
        let row = |id: &str, stats: serde_json::Value| -> ProjectionRow {
            serde_json::from_value(serde_json::json!({
                "player_id": id,
                "player": {"first_name": id, "last_name": "X", "position": "WR", "team": "KC"},
                "stats": stats
            }))
            .unwrap()
        };
        let rows = vec![
            row(
                "a",
                serde_json::json!({"rec": 100, "rec_yd": 1200, "adp_ppr": 5.0, "adp_half_ppr": 9.0}),
            ),
            row(
                "b",
                serde_json::json!({"rec": 90, "rec_yd": 1100, "adp_ppr": 12.0}),
            ),
        ];
        let rules = RosterRules::new(&league.roster_positions);
        let mut warnings = Vec::new();
        let built = build_board(
            &league,
            &draft,
            &HashMap::new(),
            &rows,
            &[],
            &rules,
            &mut warnings,
        );
        let adp_of = |id: &str| {
            built
                .players
                .iter()
                .find(|p| p.player_id == id)
                .unwrap()
                .adp
        };
        assert_eq!(adp_of("a"), Some(9.0));
        assert_eq!(adp_of("b"), Some(12.0));
    }
}
