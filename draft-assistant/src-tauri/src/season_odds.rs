//! Standings, rest-of-season point projections, and playoff odds.
//!
//! Playoff odds are a Monte Carlo over the league's real remaining schedule:
//! every team scores its best-lineup projection each week plus noise, the
//! bracket is cut the way the league cuts it, and we count how often each
//! roster lands inside it. Deterministically seeded so the same league state
//! always yields the same percentages — a number that flickers on every
//! refresh reads as broken even when it is technically correct.

use serde::Serialize;
use std::collections::HashMap;

const SIMULATIONS: usize = 4000;
/// Week-to-week scoring noise as a fraction of a team's projected mean.
/// Fantasy team scores land near 25-30% CV in practice.
const TEAM_SCORE_CV: f64 = 0.27;

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
    let games: Vec<(usize, usize, f64, f64)> = schedule
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
            Some((home, away, mean_for(game.home), mean_for(game.away)))
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
        for &(home, away, home_mean, away_mean) in &games {
            let mut score = |slot: usize, mean: f64, rng: &mut Rng| {
                let value = (mean + rng.next_normal() * mean * TEAM_SCORE_CV).max(0.0);
                points[slot] += value;
                value
            };
            let home_score = score(home, home_mean, &mut rng);
            let away_score = score(away, away_mean, &mut rng);
            if (home_score - away_score).abs() < 1e-9 {
                wins[home] += 0.5;
                wins[away] += 0.5;
            } else if home_score > away_score {
                wins[home] += 1.0;
            } else {
                wins[away] += 1.0;
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

/// Probability the home side outscores the away side in a single game.
pub fn win_probability(my_mean: f64, opp_mean: f64) -> f64 {
    if my_mean <= 0.0 && opp_mean <= 0.0 {
        return 0.5;
    }
    // Difference of two independent normals is normal; combine the variances.
    let my_sigma = (my_mean * TEAM_SCORE_CV).max(1.0);
    let opp_sigma = (opp_mean * TEAM_SCORE_CV).max(1.0);
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
mod tests {
    use super::*;

    fn team(roster_id: u32, wins: u32, losses: u32, points: f64, weekly: f64) -> TeamSeason {
        TeamSeason {
            roster_id,
            wins,
            losses,
            ties: 0,
            points_for: points,
            weekly_projection: (1..=4).map(|w| (w, weekly)).collect(),
        }
    }

    fn round_robin(ids: &[u32], weeks: u32) -> Vec<ScheduledGame> {
        let mut games = Vec::new();
        for week in 1..=weeks {
            for pair in ids.chunks(2) {
                if let [home, away] = pair {
                    games.push(ScheduledGame {
                        week,
                        home: *home,
                        away: *away,
                    });
                }
            }
        }
        games
    }

    #[test]
    fn level_teams_are_ordered_by_projection_not_roster_id() {
        let teams = vec![team(1, 0, 0, 0.0, 100.0), team(2, 0, 0, 0.0, 130.0)];
        let rows = standings(
            &teams,
            &round_robin(&[1, 2], 2),
            1,
            &|id| id.to_string(),
            None,
            1,
        );
        assert_eq!(rows[0].roster_id, 2);
        assert_eq!(rows[0].seed, 1);
        assert_eq!(rows[1].roster_id, 1);
    }

    #[test]
    fn odds_are_deterministic_for_the_same_state() {
        let teams = vec![team(1, 2, 0, 250.0, 110.0), team(2, 0, 2, 200.0, 100.0)];
        let schedule = round_robin(&[1, 2], 4);
        let a = playoff_odds(&teams, &schedule, 1, 42);
        let b = playoff_odds(&teams, &schedule, 1, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn a_better_team_with_a_better_record_makes_the_bracket_more_often() {
        let teams = vec![team(1, 3, 0, 400.0, 130.0), team(2, 0, 3, 250.0, 90.0)];
        let odds = playoff_odds(&teams, &round_robin(&[1, 2], 3), 1, 7);
        assert!(odds[&1] > 0.85, "strong team odds {:?}", odds[&1]);
        assert!(odds[&2] < 0.15, "weak team odds {:?}", odds[&2]);
    }

    #[test]
    fn every_team_makes_it_when_the_bracket_is_as_wide_as_the_league() {
        let teams = vec![team(1, 1, 1, 200.0, 100.0), team(2, 1, 1, 200.0, 100.0)];
        let odds = playoff_odds(&teams, &round_robin(&[1, 2], 2), 2, 3);
        assert!((odds[&1] - 1.0).abs() < 1e-9);
        assert!((odds[&2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn with_no_games_left_the_standings_decide_outright() {
        let teams = vec![team(1, 1, 2, 200.0, 100.0), team(2, 2, 1, 190.0, 100.0)];
        let odds = playoff_odds(&teams, &[], 1, 11);
        assert_eq!(odds[&2], 1.0);
        assert_eq!(odds[&1], 0.0);
    }

    #[test]
    fn win_probability_is_symmetric_and_ordered() {
        assert!((win_probability(110.0, 110.0) - 0.5).abs() < 1e-6);
        let favoured = win_probability(125.0, 100.0);
        assert!(favoured > 0.5 && favoured < 1.0);
        assert!((favoured + win_probability(100.0, 125.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn seeding_breaks_ties_on_points_and_flags_my_team() {
        let teams = vec![team(1, 2, 0, 210.0, 100.0), team(2, 2, 0, 260.0, 100.0)];
        let rows = standings(&teams, &[], 1, &|id| format!("team{id}"), Some(1), 5);
        assert_eq!(rows[0].roster_id, 2);
        assert_eq!(rows[0].seed, 1);
        assert_eq!(rows[1].record, "2\u{2013}0");
        assert!(rows[1].is_mine);
        assert!(!rows[0].is_mine);
    }
}
