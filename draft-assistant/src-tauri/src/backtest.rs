//! Does the win probability mean anything? Replay a finished season through
//! the same model the app shows and compare it to what happened.
//!
//! For every completed week the app's own pieces are used — the league's
//! scoring on that week's projections, [`crate::season_spread::position_cv`]
//! for each starter's spread, the stack correlation between team-mates, and
//! [`crate::season_odds::win_probability`] itself for the call — so a number
//! here is a statement about the shipped model, not about a copy of it. The
//! three things worth knowing:
//!
//! * **calibration** — of the games we called 70%, did about 70% win?
//! * **spread** — is `sigma` the right size? `z_sd` near 1.0 says yes; above
//!   1 the model is too sure of itself, below 1 too timid.
//! * **per position** — what a week around a projection really looks like,
//!   which is where [`crate::season_spread::position_cv`] came from.
//!
//! The driver that fetches a real league and prints the tables is
//! `src/bin/backtest.rs`; everything here is pure arithmetic over the rows it
//! collects, which is what makes it testable without a network.

use crate::season_odds;
use crate::season_spread::{self, position_cv, Starter};
use serde::Serialize;
use std::collections::HashMap;

/// One finished head-to-head: what the model said before kickoff and what
/// the scoreboard said after.
#[derive(Debug, Clone, Serialize)]
pub struct Game {
    pub week: u32,
    pub roster_a: u32,
    pub roster_b: u32,
    /// P(a beats b) from the projected lineups.
    pub p_a: f64,
    pub projected_a: f64,
    pub projected_b: f64,
    pub actual_a: f64,
    pub actual_b: f64,
    /// The model's spread on the margin.
    pub sigma: f64,
}

impl Game {
    pub fn a_won(&self) -> bool {
        self.actual_a > self.actual_b
    }

    /// How far off the margin was, in the model's own units.
    pub fn z(&self) -> f64 {
        let predicted = self.projected_a - self.projected_b;
        let actual = self.actual_a - self.actual_b;
        (actual - predicted) / self.sigma
    }
}

/// P(a beats b) and the spread behind it.
///
/// The probability is the shipped [`season_odds::win_probability`], called
/// with the lineups already chosen — not a re-derivation of it. Only the
/// spread has to be recomputed here, because the shipped function returns the
/// probability alone and the calibration tables need the sigma that produced
/// it. The two are combined exactly the way that function combines them, and
/// `the_reported_sigma_is_the_one_behind_the_probability` holds them to it.
pub fn win_probability(a: &[Starter], b: &[Starter]) -> (f64, f64) {
    let sigma_a = season_spread::team_sigma(a).max(1.0);
    let sigma_b = season_spread::team_sigma(b).max(1.0);
    let sigma = (sigma_a * sigma_a + sigma_b * sigma_b).sqrt();
    (season_odds::win_probability(a, b), sigma)
}

/// One row of the calibration table: the games whose call landed in this
/// band, and how many of them actually won.
#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    pub low: f64,
    pub high: f64,
    pub games: usize,
    /// Mean predicted probability in the band.
    pub predicted: f64,
    /// Share that actually won.
    pub actual: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Calibration {
    pub games: usize,
    /// Share of games the favourite won.
    pub accuracy: f64,
    /// Mean squared error of the probability. 0.25 is a coin flip.
    pub brier: f64,
    /// Mean negative log likelihood. 0.693 is a coin flip.
    pub log_loss: f64,
    /// Standard deviation of [`Game::z`]. 1.0 means `sigma` is right.
    pub z_sd: f64,
    /// Mean of [`Game::z`]: a projection bias in one side's favour would show
    /// here, but with both sides drawn from the same pool it should be ~0.
    pub z_mean: f64,
    /// Mean absolute error of a side's projected score, in points.
    pub score_mae: f64,
    pub buckets: Vec<Bucket>,
}

const BANDS: [(f64, f64); 5] = [(0.0, 0.4), (0.4, 0.5), (0.5, 0.6), (0.6, 0.7), (0.7, 1.01)];

fn empty_calibration() -> Calibration {
    Calibration {
        games: 0,
        accuracy: 0.0,
        brier: 0.0,
        log_loss: 0.0,
        z_sd: 0.0,
        z_mean: 0.0,
        score_mae: 0.0,
        buckets: Vec::new(),
    }
}

fn buckets_of(games: &[Game]) -> Vec<Bucket> {
    BANDS
        .iter()
        .map(|(low, high)| {
            let inside: Vec<&Game> = games
                .iter()
                .filter(|g| g.p_a >= *low && g.p_a < *high)
                .collect();
            let k = inside.len().max(1) as f64;
            Bucket {
                low: *low,
                high: high.min(1.0),
                games: inside.len(),
                predicted: inside.iter().map(|g| g.p_a).sum::<f64>() / k,
                actual: inside.iter().filter(|g| g.a_won()).count() as f64 / k,
            }
        })
        .collect()
}

pub fn calibrate(games: &[Game]) -> Calibration {
    let n = games.len();
    if n == 0 {
        return empty_calibration();
    }
    let count = n as f64;
    let mut hits = 0.0;
    let mut brier = 0.0;
    let mut log_loss = 0.0;
    let mut zs = Vec::with_capacity(n);
    let mut score_error = 0.0;
    for g in games {
        let won = if g.a_won() { 1.0 } else { 0.0 };
        // A call of exactly 50% is neither right nor wrong.
        if (g.p_a - 0.5).abs() > 1e-9 {
            if (g.p_a > 0.5) == g.a_won() {
                hits += 1.0;
            }
        } else {
            hits += 0.5;
        }
        brier += (g.p_a - won).powi(2);
        let p = g.p_a.clamp(1e-6, 1.0 - 1e-6);
        log_loss -= won * p.ln() + (1.0 - won) * (1.0 - p).ln();
        if g.sigma > 0.0 {
            zs.push(g.z());
        }
        score_error += (g.projected_a - g.actual_a).abs() + (g.projected_b - g.actual_b).abs();
    }
    let z_mean = zs.iter().sum::<f64>() / zs.len().max(1) as f64;
    let z_var = zs.iter().map(|z| (z - z_mean).powi(2)).sum::<f64>() / zs.len().max(1) as f64;
    Calibration {
        games: n,
        accuracy: hits / count,
        brier: brier / count,
        log_loss: log_loss / count,
        z_sd: z_var.sqrt(),
        z_mean,
        score_mae: score_error / (2.0 * count),
        buckets: buckets_of(games),
    }
}

/// Log loss if every spread were `scale` times what the model uses. The
/// margin and the outcome are fixed; only the confidence moves.
pub fn log_loss_at(games: &[Game], scale: f64) -> f64 {
    let usable: Vec<&Game> = games.iter().filter(|g| g.sigma > 0.0).collect();
    if usable.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    for g in &usable {
        let margin = g.projected_a - g.projected_b;
        let p = crate::scoring::norm_cdf(margin / (g.sigma * scale)).clamp(1e-6, 1.0 - 1e-6);
        let won = if g.a_won() { 1.0 } else { 0.0 };
        total -= won * p.ln() + (1.0 - won) * (1.0 - p).ln();
    }
    total / usable.len() as f64
}

/// The spread the season actually wanted: scan and keep the best. A scale
/// below 1 means the model hedges more than the games did, above 1 that it
/// was too sure of itself.
pub fn best_sigma_scale(games: &[Game]) -> (f64, f64) {
    let mut best = (1.0, log_loss_at(games, 1.0));
    for step in 1..=40 {
        let scale = 0.5 + 0.05 * step as f64;
        let loss = log_loss_at(games, scale);
        if loss < best.1 {
            best = (scale, loss);
        }
    }
    best
}

/// One started player's week: what the projection said, what he scored.
#[derive(Debug, Clone)]
pub struct PlayerWeek {
    pub position: String,
    pub projected: f64,
    pub actual: f64,
}

/// What a week around a projection really looks like at one position.
#[derive(Debug, Clone, Serialize)]
pub struct PositionFit {
    pub position: String,
    pub games: usize,
    /// Mean actual / mean projected. Below 1 means the projections run hot.
    pub bias: f64,
    /// Root mean square of (actual - projected) / projected: the spread the
    /// model calls `position_cv`.
    pub cv: f64,
    /// What the model currently uses.
    pub model_cv: f64,
}

/// Only players projected for a real starting week are informative: the
/// spread of a 1.2-point projection is meaningless as a ratio.
const MIN_PROJECTION: f64 = 4.0;

pub fn position_fits(weeks: &[PlayerWeek]) -> Vec<PositionFit> {
    let mut by_position: HashMap<&str, Vec<&PlayerWeek>> = HashMap::new();
    for w in weeks.iter().filter(|w| w.projected >= MIN_PROJECTION) {
        by_position.entry(&w.position).or_default().push(w);
    }
    let mut fits: Vec<PositionFit> = by_position
        .into_iter()
        .map(|(position, rows)| {
            let n = rows.len() as f64;
            let projected: f64 = rows.iter().map(|w| w.projected).sum();
            let actual: f64 = rows.iter().map(|w| w.actual).sum();
            let mean_square: f64 = rows
                .iter()
                .map(|w| ((w.actual - w.projected) / w.projected).powi(2))
                .sum::<f64>()
                / n;
            PositionFit {
                position: position.to_string(),
                games: rows.len(),
                bias: if projected > 0.0 {
                    actual / projected
                } else {
                    0.0
                },
                cv: mean_square.sqrt(),
                model_cv: position_cv(position),
            }
        })
        .collect();
    fits.sort_by(|a, b| a.position.cmp(&b.position));
    fits
}

#[cfg(test)]
mod tests;
