//! The scored draft board: players valued under the league's exact rules.

use crate::scoring;
use crate::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use crate::valuation::{self, ScoredPlayer};
use serde::Serialize;
use std::collections::HashMap;

pub const WEEKS: u32 = 18;

#[derive(Debug, Clone, Serialize)]
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


pub fn build_board(
    league: &League,
    _draft: &Draft,
    player_meta: &HashMap<String, PlayerMeta>,
    season_rows: &[ProjectionRow],
    weekly_rows: &[ProjectionRow],
    warnings: &mut Vec<String>,
) -> Vec<BoardPlayer> {
    let scoring_map = &league.scoring_settings;

    // Positions this league actually rosters (K excluded automatically for
    // this league because there is no K slot).
    let mut wanted: Vec<&str> = vec![];
    for slot in &league.roster_positions {
        match slot.as_str() {
            "QB" | "RB" | "WR" | "TE" | "DEF" | "K" => {
                if !wanted.contains(&slot.as_str()) {
                    wanted.push(slot.as_str());
                }
            }
            _ => {}
        }
    }
    for slot in &league.roster_positions {
        if let Some(elig) = valuation::flex_eligible(slot) {
            for pos in elig {
                if !wanted.contains(&pos) {
                    wanted.push(pos);
                }
            }
        }
    }

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
            weekly_by_player.entry(row.player_id.as_str()).or_default().push(stats);
        }
        if let (Some(meta), Some(week), Some(_)) = (&row.player, row.week, row.opponent.as_ref()) {
            if let Some(team) = &meta.team {
                if (1..=WEEKS).contains(&week) {
                    team_week_counts.entry(team.clone()).or_insert([0; WEEKS as usize])
                        [(week - 1) as usize] += 1;
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
        let (week_idx, &min) = counts
            .iter()
            .enumerate()
            .min_by_key(|(_, &c)| c)?;
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
        if !wanted.contains(&position.as_str()) {
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
                    if joined.is_empty() { None } else { Some(joined) }
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
            adp: row.stat("adp_ppr").filter(|&a| a > 0.0 && a < 500.0),
            injury_status: player_meta
                .get(&row.player_id)
                .and_then(|m| m.injury_status.clone()),
            sleeper_pts_ppr: row.stat("pts_ppr"),
        });
    }

    if scored.is_empty() {
        warnings.push("no scored players — projections fetch likely failed".into());
        return scored;
    }

    // Replacement + VORP.
    let as_scored: Vec<ScoredPlayer> = scored
        .iter()
        .map(|p| ScoredPlayer {
            position: p.position.clone(),
            points: p.points,
        })
        .collect();
    let model = valuation::compute_replacement(
        &as_scored,
        &league.roster_positions,
        league.total_rosters as usize,
    );
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
    scored
}

