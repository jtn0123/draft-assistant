//! Standings, rest-of-season point projections, and playoff odds.
//!
//! Playoff odds are a Monte Carlo over the league's real remaining schedule:
//! every team scores its best-lineup projection each week plus noise, the
//! bracket is cut the way the league cuts it, and we count how often each
//! roster lands inside it. Deterministically seeded so the same league state
//! always yields the same percentages — a number that flickers on every
//! refresh reads as broken even when it is technically correct.
//!
//! The noise is not a flat fraction of the projection: it is the spread the
//! week's actual starters imply, stacks and all, from
//! [`crate::season_spread`]. The same spread prices this week's win
//! probability, so the header's odds and the playoff simulation are two
//! readings of one model rather than two models that happen to disagree.

use crate::season_spread::{self, Starter};
use serde::Serialize;
use std::collections::HashMap;

const SIMULATIONS: usize = 4000;

/// Deterministic xorshift64*. Seeded from league state so odds are stable
/// across refreshes but still decorrelated between leagues.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Box-Muller, one of the pair. Standard normal.
    fn next_normal(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// One team's season-to-date record and rest-of-season outlook.
#[derive(Debug, Clone)]
pub struct TeamSeason {
    pub roster_id: u32,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub points_for: f64,
    /// Mean projected points per remaining week, from the best lineup they can
    /// field that week.
    pub weekly_projection: Vec<(u32, f64)>,
    /// Standard deviation of that week's score, from the same starters. Weeks
    /// missing here fall back to [`season_spread::fallback_sigma`].
    pub weekly_sigma: Vec<(u32, f64)>,
}

impl TeamSeason {
    pub fn projected_total(&self) -> f64 {
        self.points_for + self.weekly_projection.iter().map(|(_, p)| p).sum::<f64>()
    }
}

/// One remaining head-to-head game.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledGame {
    pub week: u32,
    pub home: u32,
    pub away: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StandingsRow {
    pub roster_id: u32,
    pub seed: u32,
    pub name: String,
    pub record: String,
    pub wins: u32,
    pub losses: u32,
    pub ties: u32,
    pub points_for: f64,
    /// Season-long point total: banked plus projected.
    pub projected_points: f64,
    /// Probability of making the playoff bracket, 0.0..=1.0.
    pub playoff_odds: f64,
    pub is_mine: bool,
}

/// One scheduled game with both sides' distributions already resolved to
/// team slots. The inner loop runs millions of times; nothing in here is
/// looked up again once the schedule is fixed.
struct Game {
    home: usize,
    away: usize,
    home_mean: f64,
    home_sigma: f64,
    away_mean: f64,
    away_sigma: f64,
}

/// Seed order: wins first, then total points — Sleeper's default tiebreak.
fn rank_key(wins: f64, points: f64) -> (i64, i64) {
    ((wins * 1000.0) as i64, (points * 100.0) as i64)
}

/// Simulate the remaining schedule and return roster_id -> playoff probability.
pub fn playoff_odds(
    teams: &[TeamSeason],
    schedule: &[ScheduledGame],
    playoff_teams: u32,
    seed: u64,
) -> HashMap<u32, f64> {
    let mut made: HashMap<u32, u32> = teams.iter().map(|t| (t.roster_id, 0)).collect();
    if teams.is_empty() {
        return HashMap::new();
    }
    let cut = (playoff_teams.max(1) as usize).min(teams.len());
    // Nothing left to play: the standings as they stand are the answer.
    if schedule.is_empty() {
        let mut order: Vec<&TeamSeason> = teams.iter().collect();
        order.sort_by_key(|t| std::cmp::Reverse(rank_key(t.wins as f64, t.points_for)));
        return order
            .iter()
            .enumerate()
            .map(|(i, t)| (t.roster_id, if i < cut { 1.0 } else { 0.0 }))
            .collect();
    }

    let means: HashMap<u32, HashMap<u32, f64>> = teams
        .iter()
        .map(|t| (t.roster_id, t.weekly_projection.iter().copied().collect()))
        .collect();
    let sigmas: HashMap<u32, HashMap<u32, f64>> = teams
        .iter()
        .map(|t| (t.roster_id, t.weekly_sigma.iter().copied().collect()))
        .collect();
    // A team with no projection at all still has to score something, or it
    // would be mathematically eliminated by a data gap.
    let league_mean = {
        let all: Vec<f64> = teams
            .iter()
            .flat_map(|t| t.weekly_projection.iter().map(|(_, p)| *p))
            .filter(|p| *p > 0.0)
            .collect();
        if all.is_empty() {
            100.0
        } else {
            all.iter().sum::<f64>() / all.len() as f64
        }
    };

    // Everything below indexes teams by slot rather than roster id. The inner
    // loop runs SIMULATIONS x schedule times — millions of lookups — and a
    // hash of a u32 is pure overhead once the mapping is fixed.
    let slot_of: HashMap<u32, usize> = teams
        .iter()
        .enumerate()
        .map(|(i, t)| (t.roster_id, i))
        .collect();
    // Per-slot mean for each scheduled game, resolved once instead of per
    // simulation: the schedule does not change between runs.
    let games: Vec<Game> = schedule
        .iter()
        .filter_map(|game| {
            let home = *slot_of.get(&game.home)?;
            let away = *slot_of.get(&game.away)?;
            let mean_for = |roster_id: u32| {
                means
                    .get(&roster_id)
                    .and_then(|w| w.get(&game.week).copied())
                    .filter(|m| *m > 0.0)
                    .unwrap_or(league_mean)
            };
            // A week with no starters resolved — a data gap, or a mean that
            // fell back to the league average — still needs a spread, and it
            // has to be the one an ordinary lineup would have produced.
            let sigma_for = |roster_id: u32, mean: f64| {
                sigmas
                    .get(&roster_id)
                    .and_then(|w| w.get(&game.week).copied())
                    .filter(|s| *s > 0.0)
                    .unwrap_or_else(|| season_spread::fallback_sigma(mean))
            };
            let home_mean = mean_for(game.home);
            let away_mean = mean_for(game.away);
            Some(Game {
                home,
                away,
                home_mean,
                home_sigma: sigma_for(game.home, home_mean),
                away_mean,
                away_sigma: sigma_for(game.away, away_mean),
            })
        })
        .collect();

    let start_wins: Vec<f64> = teams
        .iter()
        .map(|t| t.wins as f64 + t.ties as f64 * 0.5)
        .collect();
    let start_points: Vec<f64> = teams.iter().map(|t| t.points_for).collect();

    let mut rng = Rng::new(seed);
    let mut wins = vec![0.0f64; teams.len()];
    let mut points = vec![0.0f64; teams.len()];
    let mut made_count = vec![0u32; teams.len()];
    let mut order: Vec<usize> = (0..teams.len()).collect();

    for _ in 0..SIMULATIONS {
        wins.copy_from_slice(&start_wins);
        points.copy_from_slice(&start_points);
        for game in &games {
            let mut score = |slot: usize, mean: f64, sigma: f64, rng: &mut Rng| {
                let value = (mean + rng.next_normal() * sigma).max(0.0);
                points[slot] += value;
                value
            };
            let home_score = score(game.home, game.home_mean, game.home_sigma, &mut rng);
            let away_score = score(game.away, game.away_mean, game.away_sigma, &mut rng);
            if (home_score - away_score).abs() < 1e-9 {
                wins[game.home] += 0.5;
                wins[game.away] += 0.5;
            } else if home_score > away_score {
                wins[game.home] += 1.0;
            } else {
                wins[game.away] += 1.0;
            }
        }
        order.sort_by_key(|&slot| std::cmp::Reverse(rank_key(wins[slot], points[slot])));
        for &slot in order.iter().take(cut) {
            made_count[slot] += 1;
        }
        // sort_by_key leaves `order` permuted; reset so the next simulation
        // starts from the same ordering and the tie-break stays deterministic.
        for (slot, entry) in order.iter_mut().enumerate() {
            *entry = slot;
        }
    }

    for (slot, team) in teams.iter().enumerate() {
        made.insert(team.roster_id, made_count[slot]);
    }
    made.into_iter()
        .map(|(id, count)| (id, count as f64 / SIMULATIONS as f64))
        .collect()
}

/// Probability my side outscores theirs in a single game, from the two
/// lineups themselves.
///
/// The spread comes from the starters actually in each lineup — a defense
/// swings further than a quarterback, and two starters sharing an NFL team
/// swing together — which is the same spread the playoff simulation draws
/// with. Passing means alone would have to guess a team-wide fraction, and
/// that guess is what this replaces.
pub fn win_probability(mine: &[Starter], theirs: &[Starter]) -> f64 {
    let my_mean = season_spread::total_points(mine);
    let opp_mean = season_spread::total_points(theirs);
    if my_mean <= 0.0 && opp_mean <= 0.0 {
        return 0.5;
    }
    // Difference of two independent normals is normal; combine the variances.
    // A floor of one point keeps a lineup of a single certain starter from
    // reading as a coin flip decided by floating-point dust.
    let my_sigma = season_spread::team_sigma(mine).max(1.0);
    let opp_sigma = season_spread::team_sigma(theirs).max(1.0);
    let sigma = (my_sigma * my_sigma + opp_sigma * opp_sigma).sqrt();
    crate::scoring::norm_cdf((my_mean - opp_mean) / sigma)
}

/// Assemble the standings table, seeded and with playoff odds attached.
pub fn standings(
    teams: &[TeamSeason],
    schedule: &[ScheduledGame],
    playoff_teams: u32,
    name_of: &impl Fn(u32) -> String,
    my_roster_id: Option<u32>,
    seed: u64,
) -> Vec<StandingsRow> {
    let odds = playoff_odds(teams, schedule, playoff_teams, seed);
    let mut order: Vec<&TeamSeason> = teams.iter().collect();
    // Record, then points scored, then — before any games are played, when
    // both are level — the stronger projected roster ranks higher rather than
    // whoever happens to have the lower roster id.
    order.sort_by(|a, b| {
        rank_key(b.wins as f64, b.points_for)
            .cmp(&rank_key(a.wins as f64, a.points_for))
            .then_with(|| b.projected_total().total_cmp(&a.projected_total()))
    });
    order
        .into_iter()
        .enumerate()
        .map(|(i, t)| StandingsRow {
            roster_id: t.roster_id,
            seed: i as u32 + 1,
            name: name_of(t.roster_id),
            record: if t.ties > 0 {
                format!("{}\u{2013}{}\u{2013}{}", t.wins, t.losses, t.ties)
            } else {
                format!("{}\u{2013}{}", t.wins, t.losses)
            },
            wins: t.wins,
            losses: t.losses,
            ties: t.ties,
            points_for: t.points_for,
            projected_points: t.projected_total(),
            playoff_odds: odds.get(&t.roster_id).copied().unwrap_or(0.0),
            is_mine: my_roster_id == Some(t.roster_id),
        })
        .collect()
}

#[cfg(test)]
mod tests;
