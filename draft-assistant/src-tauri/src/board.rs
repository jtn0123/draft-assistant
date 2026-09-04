//! The scored draft board: players valued under the league's exact rules.

use crate::roster::RosterRules;
use crate::scoring;
use crate::second_opinion::SecondOpinion;
use crate::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use crate::valuation::{self, ReplacementModel, ScoredPlayer};
use crate::view::TierAlert;
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
    /// What an imported projections file says about him, when one is loaded.
    /// Ranks only -- see `second_opinion.rs` for why the points are dropped.
    pub second_opinion: Option<SecondOpinion>,
    /// Week-to-week spread of his own weekly projections, as a coefficient of
    /// variation. The upside mode's only real dispersion signal; `None` for a
    /// player with too few scoring weeks to measure.
    ///
    /// Deliberately not on the wire. Every other field here is something the
    /// screen shows or the model reads, and this one is an input to a score
    /// neither of them recomputes -- putting it in `DraftView` would move the
    /// schema for a number nobody downstream reads.
    #[serde(skip)]
    pub weekly_cv: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailablePlayer {
    #[serde(flatten)]
    pub player: BoardPlayer,
    /// P(still available at my next pick after the current one).
    pub survival_next: Option<f64>,
}

pub struct BoardBuild {
    pub players: Vec<BoardPlayer>,
    pub replacement: ReplacementModel,
}

/// How much VORP a kicker or a defence is docked before the overall board is
/// ranked. See `build_board`.
pub const ONESIE_RANK_DISCOUNT: f64 = 12.0;

/// Spread of a player's weekly projections around their own mean, as a
/// coefficient of variation. Bye weeks and any other blank week are left out;
/// under four scoring weeks is too little to measure and reads as no signal.
fn weekly_cv(
    weeks: &[&HashMap<String, f64>],
    scoring: &HashMap<String, f64>,
    position: &str,
) -> Option<f64> {
    let points: Vec<f64> = weeks
        .iter()
        .map(|week| scoring::base_points_for(week, scoring, position))
        .filter(|p| *p > 0.0)
        .collect();
    if points.len() < 4 {
        return None;
    }
    let mean = points.iter().sum::<f64>() / points.len() as f64;
    if mean <= 0.0 {
        return None;
    }
    let variance = points.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / points.len() as f64;
    Some(variance.sqrt() / mean)
}

/// True for the two positions that are always there in the last two rounds.
pub fn is_late_only(position: &str) -> bool {
    position == "K" || position == "DEF"
}

/// What a player sorts on for overall rank — VORP, discounted for K and DEF.
fn board_rank_value(player: &BoardPlayer) -> f64 {
    if is_late_only(&player.position) {
        player.vorp - ONESIE_RANK_DISCOUNT
    } else {
        player.vorp
    }
}

/// Which ADP column of a Sleeper projection row this league actually drafts
/// on. Sleeper publishes four; the board used to read PPR whatever the league
/// scored, which in a half-PPR league moved receivers a round or more away
/// from where they really go, and in a superflex league was off by a whole
/// position.
pub fn adp_key(league: &League) -> &'static str {
    let two_qb = league
        .roster_positions
        .iter()
        .any(|slot| slot == "SUPER_FLEX")
        || league
            .roster_positions
            .iter()
            .filter(|s| *s == "QB")
            .count()
            >= 2;
    if two_qb {
        return "adp_2qb";
    }
    match league.scoring_settings.get("rec").copied().unwrap_or(0.0) {
        rec if rec >= 0.75 => "adp_ppr",
        rec if rec >= 0.25 => "adp_half_ppr",
        _ => "adp_std",
    }
}

/// Sleeper publishes four ADP columns and they are not the same number: in a
/// superflex league `adp_2qb` puts quarterbacks a round and a half ahead of
/// where `adp_ppr` has them, and in a standard league receivers sit a round
/// behind. A per-row fallback straight to `adp_ppr` therefore built a board
/// where most players were priced in the league's own market and a scattered
/// minority were priced in somebody else's — invisible, and worse than no ADP
/// at all, because every downstream reader (survival, the reach discipline,
/// the falling-value bonus) compares those numbers with each other.
///
/// So the fallback is put on the league's scale first: the median ratio
/// between the two columns across every row that carries both. Identity when
/// the league already drafts on `adp_ppr`, and identity when too few rows
/// overlap to measure a ratio at all.
fn adp_fallback_scale(rows: &[ProjectionRow], adp_key: &str) -> f64 {
    if adp_key == "adp_ppr" {
        return 1.0;
    }
    let usable = |adp: f64| adp > 0.0 && adp < 500.0;
    let mut ratios: Vec<f64> = rows
        .iter()
        .filter_map(|row| {
            let league = row.stat(adp_key).filter(|&a| usable(a))?;
            let ppr = row.stat("adp_ppr").filter(|&a| usable(a))?;
            Some(league / ppr)
        })
        .collect();
    if ratios.len() < 20 {
        return 1.0;
    }
    ratios.sort_by(f64::total_cmp);
    ratios[ratios.len() / 2]
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
    let adp_key = adp_key(league);
    let adp_scale = adp_fallback_scale(season_rows, adp_key);
    let mut adp_borrowed = 0usize;

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
    // A week with no rows for ANY team is a week that did not download, not a
    // week the whole league had off. One failed weekly fetch used to make that
    // week look like the emptiest one for all 32 teams, so every player on the
    // board came back with the same bye — and `min_by_key` handed out the
    // first such week even when a real bye tied with it.
    let mut league_week_counts = [0u32; WEEKS as usize];
    for counts in team_week_counts.values() {
        for (week_idx, count) in counts.iter().enumerate() {
            league_week_counts[week_idx] += count;
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
            .filter(|(week_idx, _)| league_week_counts[*week_idx] > 0)
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
        if !wanted.contains(&position) {
            continue;
        }
        // Names: whatever the row embeds, then Sleeper's player dictionary,
        // then first/last, then the raw id so a row is never nameless. A
        // defence used to get only the embedded half of that chain, so a DEF
        // row that arrived without player meta rendered as a blank name on the
        // board, in the rosters and in every chat context built off them.
        let joined_name = || {
            let first = meta.and_then(|m| m.first_name.clone()).unwrap_or_default();
            let last = meta.and_then(|m| m.last_name.clone()).unwrap_or_default();
            let joined = format!("{first} {last}").trim().to_string();
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        };
        let name = player_meta
            .get(&row.player_id)
            .and_then(|m| m.full_name.clone())
            .filter(|full| !full.trim().is_empty())
            .or_else(joined_name)
            .unwrap_or_else(|| row.player_id.clone());
        let base = scoring::base_points_for(stats, scoring_map, &position);
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
                .stat(adp_key)
                .filter(|&a| a > 0.0 && a < 500.0)
                .or_else(|| {
                    let borrowed = row
                        .stat("adp_ppr")
                        .filter(|&a| a > 0.0 && a < 500.0)
                        .map(|ppr| ppr * adp_scale);
                    adp_borrowed += usize::from(borrowed.is_some());
                    borrowed
                }),
            injury_status: player_meta
                .get(&row.player_id)
                .and_then(|m| m.injury_status.clone()),
            sleeper_pts_ppr: row.stat("pts_ppr"),
            second_opinion: None,
            weekly_cv: weekly_by_player
                .get(row.player_id.as_str())
                .and_then(|weeks| weekly_cv(weeks, scoring_map, &position)),
        });
    }

    // Said out loud, because a board where some ADPs came off another column
    // is a board whose market numbers are only as good as one median ratio.
    // Silent when the ratio came out at one: nothing was actually restated,
    // and a warning about a no-op is a warning the user learns to skip.
    if adp_borrowed > 0 && (adp_scale - 1.0).abs() > 0.01 {
        warnings.push(format!(
            "{adp_borrowed} players have no {adp_key} ADP; theirs is PPR ADP rescaled by {adp_scale:.2}"
        ));
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
    // `None` takes `valuation::DEFAULT_FLEX_BIAS`. Nothing reads a per-league
    // override yet — there is no league-rules or house-rules store to put one
    // in — so the knob is the argument, not a setting.
    let model =
        valuation::compute_replacement(&as_scored, rules, league.total_rosters as usize, None);
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

    // Overall rank by VORP, less a flat discount for the two positions whose
    // VORP lies about them. A kicker or a defence really is worth ~20 points
    // over replacement, but that value is available to anybody in the last two
    // rounds, so ranking on raw VORP put the best defence at #64 — three
    // rounds ahead of its own ADP, and above sixty players who cannot be had
    // late. The discount is one round's worth of VORP at the depth these come
    // off the board, which lands them back in their ADP band.
    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_by(|&a, &b| {
        board_rank_value(&scored[b])
            .partial_cmp(&board_rank_value(&scored[a]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (rank, &i) in order.iter().enumerate() {
        scored[i].overall_rank = rank as u32 + 1;
    }
    scored.sort_by_key(|p| p.overall_rank);
    BoardBuild {
        players: scored,
        replacement: model,
    }
}

/// The best tier still on the board at each position, and how many players are
/// left in it — one alert per draftable position that has anyone left, in the
/// order the league rosters them.
///
/// `available` is in board order, which is ranked and not tier-sorted: a
/// tier-1 running back can sit below a tier-2 one, and after a run on a
/// position the first player left at it is often not from the best tier still
/// there. So the best tier is the smallest one seen, and the count is of that
/// tier — taking the first player's tier as the answer reported "RB T2 has 3
/// left" while three tier-1 backs were still on the board. Still one pass:
/// a better tier resets the count, a worse one is ignored.
pub fn tier_alerts(available: &[AvailablePlayer], positions: Vec<String>) -> Vec<TierAlert> {
    let mut top: HashMap<&str, (u32, u32)> = HashMap::new();
    for a in available {
        let entry = top
            .entry(a.player.position.as_str())
            .or_insert((a.player.tier, 0));
        match a.player.tier.cmp(&entry.0) {
            std::cmp::Ordering::Less => *entry = (a.player.tier, 1),
            std::cmp::Ordering::Equal => entry.1 += 1,
            std::cmp::Ordering::Greater => {}
        }
    }
    positions
        .into_iter()
        .filter_map(|pos| {
            top.get(pos.as_str())
                .map(|&(tier, players_left)| TierAlert {
                    position: pos,
                    tier,
                    players_left,
                })
        })
        .collect()
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
