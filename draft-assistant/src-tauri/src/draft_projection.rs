//! Who the draft says wins.
//!
//! Every other number on the draft screen is about the next pick. This is
//! about the season the picks are adding up to: each roster's best starting
//! lineup, scored on this league's own rules, and how often that roster comes
//! out on top once the same week-to-week spread the season screen prices
//! matchups with is applied to it.
//!
//! Three things it is not. It is not a grade: nobody is told their draft was a
//! B+. It is not the season model — there is no schedule during a draft, so
//! what is simulated is total points, and "wins the league" means finishing
//! first on points rather than lifting a trophy. And it is not settled until
//! the draft is: a roster with four starting slots still empty projects like a
//! roster with four zeroes in it, which is exactly what it is until those
//! picks are made, so the empty slots are named beside the number.

use crate::roster::RosterRules;
use crate::season_spread;
use serde::Serialize;

/// Simulations behind `title_odds`. The same count the season's playoff odds
/// use, for the same reason: enough that the third decimal stops moving.
const SIMULATIONS: usize = 4000;

/// Games a full season projection is spread over. The per-week spread model
/// wants a week's points, and what a board carries is a season's.
const GAMES: f64 = 17.0;

/// Regular-season weeks the total is accumulated over. Week-to-week results
/// are taken as independent, so a season's spread is one week's times the
/// root of this — the usual random-walk widening, and the reason a projected
/// lead of thirty points is not the lock it looks like.
const WEEKS: f64 = 14.0;

/// One roster, and the season the draft has bought it so far.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TeamProjection {
    pub slot: u32,
    pub name: Option<String>,
    /// Best starting lineup's season points, on this league's scoring.
    pub starters: f64,
    /// Everyone else drafted, at their own season points. Depth, not lineup.
    pub bench: f64,
    /// Starting slots nobody on this roster can fill yet, in roster order.
    pub holes: Vec<String>,
    /// 1 for the roster projecting the most starting points.
    pub rank: u32,
    /// Share of simulations this roster finishes first on points. Unavailable
    /// when no roster has projected starters: an empty draft has no favorite.
    pub title_odds: Option<f64>,
    pub is_mine: bool,
}

/// One drafted player, as a projection needs him.
pub struct Drafted<'a> {
    pub player_id: &'a str,
    pub position: &'a str,
    pub team: Option<&'a str>,
    /// Season points under this league's scoring, or `None` for a player the
    /// board has no projection for (a rookie nobody has priced, an id that
    /// did not match). He fills no slot rather than filling one with a zero.
    pub points: Option<f64>,
}

/// One roster's players, in the order they were drafted.
pub struct Team<'a> {
    pub slot: u32,
    pub name: Option<String>,
    pub is_mine: bool,
    pub players: Vec<Drafted<'a>>,
}

/// The best starting lineup this roster can field, and what is left over.
///
/// Fixed slots are filled before flex ones, each taking the highest-scoring
/// player it can still have. Filling flex first would hand a FLEX the best
/// running back and leave RB2 to a worse one, which no manager would do.
fn fill(rules: &RosterRules, players: &[Drafted<'_>]) -> (Vec<usize>, Vec<String>) {
    let mut taken = vec![false; players.len()];
    let mut lineup: Vec<usize> = Vec::new();
    let mut holes: Vec<String> = Vec::new();
    let mut slots: Vec<&String> = rules
        .slots()
        .iter()
        .filter(|slot| RosterRules::counts_as_open_starter(slot))
        .collect();
    // Fill narrower flex eligibility first, even when SUPER_FLEX is listed
    // before FLEX. Otherwise it can consume the only useful FLEX player.
    slots.sort_by_key(|slot| RosterRules::flex_eligible(slot).map_or(0, <[&str]>::len));
    let flex_last = slots
        .iter()
        .map(|slot| RosterRules::flex_eligible(slot).is_some())
        .collect::<Vec<_>>();
    for pass in [false, true] {
        for (at, slot) in slots.iter().enumerate() {
            if flex_last[at] != pass {
                continue;
            }
            let best = players
                .iter()
                .enumerate()
                .filter(|(index, player)| {
                    !taken[*index]
                        && player.points.is_some()
                        && RosterRules::can_fill(slot, player.position)
                })
                .max_by(|(_, a), (_, b)| {
                    a.points
                        .unwrap_or_default()
                        .total_cmp(&b.points.unwrap_or_default())
                });
            match best {
                Some((index, _)) => {
                    taken[index] = true;
                    lineup.push(index);
                }
                None => holes.push((*slot).clone()),
            }
        }
    }
    holes.sort_by_key(|slot| rules.slots().iter().position(|original| original == slot));
    (lineup, holes)
}

/// A team's week-to-week spread, from the same model the season screen uses.
///
/// The starters are handed over at a week's worth of points each, because
/// that is the scale [`season_spread`] was calibrated on; the season's spread
/// is that widened over [`WEEKS`].
fn season_sigma(players: &[Drafted<'_>], lineup: &[usize], starters: f64) -> f64 {
    let weekly: Vec<season_spread::Starter> = lineup
        .iter()
        .map(|at| {
            let player = &players[*at];
            let points = player.points.unwrap_or_default() / GAMES;
            season_spread::Starter {
                position: player.position.to_string(),
                team: player.team.map(str::to_string),
                points,
                uncertain: points,
            }
        })
        .collect();
    let week = if weekly.is_empty() {
        season_spread::fallback_sigma(starters / WEEKS)
    } else {
        season_spread::team_sigma(&weekly)
    };
    week * WEEKS.sqrt()
}

/// Deterministic xorshift64*, seeded from the rosters so the same board always
/// gives the same percentages. A number that flickers on every repaint reads
/// as broken even when it is honest.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Standard normal, Box-Muller on two uniforms.
    fn normal(&mut self) -> f64 {
        let a = ((self.next() >> 11) as f64 / (1u64 << 53) as f64).max(f64::MIN_POSITIVE);
        let b = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

/// How often each roster finishes first on points.
fn title_odds(means: &[f64], sigmas: &[f64]) -> Vec<f64> {
    if means.is_empty() {
        return Vec::new();
    }
    let seed = means
        .iter()
        .fold(0x9E37_79B9_7F4A_7C15u64, |acc, mean| {
            acc.rotate_left(7) ^ (mean.to_bits())
        })
        .max(1);
    let mut rng = Rng(seed);
    let mut wins = vec![0.0; means.len()];
    let mut leaders = Vec::with_capacity(means.len());
    for _ in 0..SIMULATIONS {
        let mut best = f64::NEG_INFINITY;
        leaders.clear();
        for (index, (mean, sigma)) in means.iter().zip(sigmas).enumerate() {
            let total = mean + sigma * rng.normal();
            if total > best {
                best = total;
                leaders.clear();
                leaders.push(index);
            } else if total == best {
                leaders.push(index);
            }
        }
        let share = 1.0 / leaders.len() as f64;
        for &index in &leaders {
            wins[index] += share;
        }
    }
    wins.into_iter()
        .map(|won| won / SIMULATIONS as f64)
        .collect()
}

/// Project every roster in the draft, best first.
pub fn project(rules: &RosterRules, teams: &[Team<'_>]) -> Vec<TeamProjection> {
    let mut rows: Vec<TeamProjection> = Vec::with_capacity(teams.len());
    let mut sigmas: Vec<f64> = Vec::with_capacity(teams.len());
    for team in teams {
        let (lineup, holes) = fill(rules, &team.players);
        let starters: f64 = lineup
            .iter()
            .map(|at| team.players[*at].points.unwrap_or_default())
            .sum();
        let bench: f64 = team
            .players
            .iter()
            .enumerate()
            .filter(|(index, _)| !lineup.contains(index))
            .map(|(_, player)| player.points.unwrap_or_default())
            .sum();
        sigmas.push(season_sigma(&team.players, &lineup, starters));
        rows.push(TeamProjection {
            slot: team.slot,
            name: team.name.clone(),
            starters,
            bench,
            holes,
            rank: 0,
            title_odds: None,
            is_mine: team.is_mine,
        });
    }
    let means: Vec<f64> = rows.iter().map(|row| row.starters).collect();
    if means.iter().any(|mean| *mean > 0.0) {
        for (row, odds) in rows.iter_mut().zip(title_odds(&means, &sigmas)) {
            row.title_odds = Some(odds);
        }
    }
    rows.sort_by(|a, b| b.starters.total_cmp(&a.starters));
    for (at, row) in rows.iter_mut().enumerate() {
        row.rank = at as u32 + 1;
    }
    rows
}

#[cfg(test)]
#[path = "draft_projection_tests.rs"]
mod tests;
