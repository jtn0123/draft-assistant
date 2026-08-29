//! Playoff odds: the rest of the season played out on the league's own
//! schedule, many times over.
//!
//! Each remaining week every team scores its best lineup's projection plus
//! noise (per-starter spread, the same one the matchup preview uses), head
//! to head against the opponent Sleeper has scheduled, and — when the
//! league plays one — against the league average that week. Records carry
//! over from what has been played. At the end the top `playoff_teams` by
//! record, points as the tiebreak, are in. Two thousand seasons, seeded, so
//! two loads on the same data agree.

use crate::draft::TeamRoster;
use crate::lineup::{self, Candidate};
use crate::loaded::LoadedLeague;
use crate::sleeper::Matchup;
use serde::Serialize;
use std::collections::HashMap;

const RUNS: usize = 2000;
/// Week-to-week spread of a starter around his projection, as a fraction.
const PLAYER_CV: f64 = 0.5;
/// Fantasy regular season, and the weeks a season projection is spread over.
const WEEKS: u32 = 17;

#[derive(Debug, Clone, Serialize)]
pub struct TeamOdds {
    pub slot: u32,
    pub display_name: Option<String>,
    /// 0..1
    pub playoff_odds: f64,
    pub expected_wins: f64,
    pub expected_points: f64,
    pub runs: u32,
}

/// xorshift64*: dependency-free, deterministic, plenty for a season sim.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in (0, 1].
    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// Standard normal, Box–Muller.
    fn normal(&mut self) -> f64 {
        let u = self.unit();
        let v = self.unit();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// A team's expected score and spread for one week.
#[derive(Clone, Copy)]
struct WeekStrength {
    mean: f64,
    sigma: f64,
}

fn strength(
    season: &[Candidate],
    weekly: &HashMap<String, Vec<(u32, f64)>>,
    week: u32,
    rules: &crate::roster::RosterRules,
) -> WeekStrength {
    let from_rows = lineup::week_candidates(season, weekly, week);
    let candidates: Vec<Candidate> = if from_rows.is_empty() {
        season
            .iter()
            .filter(|c| c.bye_week != Some(week))
            .map(|c| Candidate {
                points: c.points / f64::from(WEEKS),
                ..c.clone()
            })
            .collect()
    } else {
        from_rows
    };
    let (mean, starters) = lineup::best_lineup(&candidates, rules);
    let sigma = starters
        .iter()
        .map(|s| (PLAYER_CV * s.points).powi(2))
        .sum::<f64>()
        .sqrt();
    WeekStrength { mean, sigma }
}

struct Standing {
    wins: f64,
    points: f64,
}

pub fn simulate(loaded: &LoadedLeague, rosters: &[TeamRoster], from_week: u32) -> Vec<TeamOdds> {
    let settings = &loaded.league.settings;
    let last_regular = settings.playoff_week_start.saturating_sub(1).max(1);
    let remaining: Vec<&(u32, Vec<Matchup>)> = loaded
        .schedule
        .iter()
        .filter(|(w, m)| *w >= from_week && *w <= last_regular && !m.is_empty())
        .collect();
    if remaining.is_empty() || rosters.is_empty() {
        return Vec::new();
    }
    let slot_to_roster: HashMap<u32, u32> = loaded
        .draft
        .slot_to_roster_id
        .as_ref()
        .map(|m| {
            m.iter()
                .filter_map(|(s, r)| s.parse().ok().map(|s: u32| (s, *r)))
                .collect()
        })
        .unwrap_or_default();
    let roster_to_index: HashMap<u32, usize> = rosters
        .iter()
        .enumerate()
        .filter_map(|(i, r)| Some((*slot_to_roster.get(&r.slot)?, i)))
        .collect();
    if roster_to_index.is_empty() {
        return Vec::new();
    }
    let n = rosters.len();
    // Per team per remaining week: mean and sigma.
    let seasons: Vec<Vec<Candidate>> = rosters
        .iter()
        .map(|r| lineup::season_candidates(r, &loaded.board, &loaded.board_index))
        .collect();
    let strengths: Vec<Vec<WeekStrength>> = remaining
        .iter()
        .map(|(week, _)| {
            seasons
                .iter()
                .map(|s| strength(s, &loaded.weekly_points, *week, &loaded.roster_rules))
                .collect()
        })
        .collect();
    // Pairings per remaining week as team-index pairs.
    let pairings: Vec<Vec<(usize, usize)>> = remaining
        .iter()
        .map(|(_, matchups)| {
            let mut by_id: HashMap<u32, Vec<usize>> = HashMap::new();
            for m in matchups {
                if let (Some(id), Some(&i)) = (m.matchup_id, roster_to_index.get(&m.roster_id)) {
                    by_id.entry(id).or_default().push(i);
                }
            }
            by_id
                .into_values()
                .filter(|v| v.len() == 2)
                .map(|v| (v[0], v[1]))
                .collect()
        })
        .collect();
    // What has already been played, from Sleeper's records.
    let base: Vec<Standing> = rosters
        .iter()
        .map(|r| {
            let rec = slot_to_roster
                .get(&r.slot)
                .and_then(|rid| loaded.league_rosters.iter().find(|x| x.roster_id == *rid));
            Standing {
                wins: rec.map_or(0.0, |x| {
                    f64::from(x.settings.wins) + 0.5 * f64::from(x.settings.ties)
                }),
                points: rec.map_or(0.0, |x| {
                    f64::from(x.settings.fpts) + f64::from(x.settings.fpts_decimal) / 100.0
                }),
            }
        })
        .collect();
    let league_average = settings.league_average_match > 0;
    let playoff_teams = (settings.playoff_teams.max(1) as usize).min(n);

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut made = vec![0u32; n];
    let mut wins_sum = vec![0.0; n];
    let mut points_sum = vec![0.0; n];
    let mut scores = vec![0.0; n];
    for _ in 0..RUNS {
        let mut wins: Vec<f64> = base.iter().map(|b| b.wins).collect();
        let mut points: Vec<f64> = base.iter().map(|b| b.points).collect();
        for (wk, pairs) in pairings.iter().enumerate() {
            for (i, s) in strengths[wk].iter().enumerate() {
                scores[i] = (s.mean + s.sigma * rng.normal()).max(0.0);
                points[i] += scores[i];
            }
            for &(a, b) in pairs {
                if scores[a] > scores[b] {
                    wins[a] += 1.0;
                } else if scores[b] > scores[a] {
                    wins[b] += 1.0;
                } else {
                    wins[a] += 0.5;
                    wins[b] += 0.5;
                }
            }
            if league_average {
                let avg = scores.iter().sum::<f64>() / n as f64;
                for i in 0..n {
                    if scores[i] > avg {
                        wins[i] += 1.0;
                    }
                }
            }
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| {
            wins[b]
                .total_cmp(&wins[a])
                .then(points[b].total_cmp(&points[a]))
        });
        for &i in order.iter().take(playoff_teams) {
            made[i] += 1;
        }
        for i in 0..n {
            wins_sum[i] += wins[i];
            points_sum[i] += points[i];
        }
    }
    let mut out: Vec<TeamOdds> = rosters
        .iter()
        .enumerate()
        .map(|(i, r)| TeamOdds {
            slot: r.slot,
            display_name: r.display_name.clone(),
            playoff_odds: f64::from(made[i]) / RUNS as f64,
            expected_wins: wins_sum[i] / RUNS as f64,
            expected_points: points_sum[i] / RUNS as f64,
            runs: RUNS as u32,
        })
        .collect();
    out.sort_by(|a, b| b.playoff_odds.total_cmp(&a.playoff_odds));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rng_is_deterministic_and_roughly_normal() {
        let mut a = Rng(42);
        let mut b = Rng(42);
        assert_eq!(a.next_u64(), b.next_u64());
        let mut r = Rng(7);
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.03, "{mean}");
        assert!((var - 1.0).abs() < 0.05, "{var}");
    }
}
