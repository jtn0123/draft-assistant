//! Does the win probability mean anything? Replay a finished season through
//! the same model the app shows and compare it to what happened.
//!
//! For every completed week the app's own pieces are used — the league's
//! scoring on that week's projections, `position_cv` for each starter's
//! spread, the stack correlation between team-mates — so a number here is a
//! statement about the shipped model, not about a copy of it. The three
//! things worth knowing:
//!
//! * **calibration** — of the games we called 70%, did about 70% win?
//! * **spread** — is `sigma` the right size? `z_sd` near 1.0 says yes; above
//!   1 the model is too sure of itself, below 1 too timid.
//! * **per position** — what a week around a projection really looks like,
//!   which is where `position_cv` came from as a guess.

use crate::lineup::Starter;
use crate::matchup::{position_cv, team_variance, Teams};
use crate::scoring::norm_cdf;
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

/// P(a beats b) and the spread behind it — the app's `preview` math with the
/// lineups already chosen.
pub fn win_probability(a: &[Starter], b: &[Starter], teams: &Teams) -> (f64, f64) {
    let margin: f64 =
        a.iter().map(|s| s.points).sum::<f64>() - b.iter().map(|s| s.points).sum::<f64>();
    let sigma = crate::matchup::SPREAD_CALIBRATION
        * (team_variance(a, teams) + team_variance(b, teams)).sqrt();
    if sigma <= 0.0 {
        return (if margin > 0.0 { 1.0 } else { 0.5 }, 0.0);
    }
    (norm_cdf(margin / sigma), sigma)
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
    /// Standard deviation of `Game::z`. 1.0 means `sigma` is right.
    pub z_sd: f64,
    /// Mean of `Game::z`: a projection bias in one side's favour would show
    /// here, but with both sides drawn from the same pool it should be ~0.
    pub z_mean: f64,
    /// Mean absolute error of a side's projected score, in points.
    pub score_mae: f64,
    pub buckets: Vec<Bucket>,
}

const BANDS: [(f64, f64); 5] = [(0.0, 0.4), (0.4, 0.5), (0.5, 0.6), (0.6, 0.7), (0.7, 1.01)];

pub fn calibrate(games: &[Game]) -> Calibration {
    let n = games.len();
    if n == 0 {
        return Calibration {
            games: 0,
            accuracy: 0.0,
            brier: 0.0,
            log_loss: 0.0,
            z_sd: 0.0,
            z_mean: 0.0,
            score_mae: 0.0,
            buckets: Vec::new(),
        };
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
            let favoured_a = g.p_a > 0.5;
            if favoured_a == g.a_won() {
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
    let buckets = BANDS
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
        .collect();
    Calibration {
        games: n,
        accuracy: hits / count,
        brier: brier / count,
        log_loss: log_loss / count,
        z_sd: z_var.sqrt(),
        z_mean,
        score_mae: score_error / (2.0 * count),
        buckets,
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
        let p = norm_cdf(margin / (g.sigma * scale)).clamp(1e-6, 1.0 - 1e-6);
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
mod tests {
    use super::*;

    fn starter(id: &str, position: &str, points: f64) -> Starter {
        Starter {
            slot: position.to_string(),
            player_id: id.to_string(),
            name: id.to_string(),
            position: position.to_string(),
            points,
            injury: None,
        }
    }

    fn game(p_a: f64, a_won: bool) -> Game {
        Game {
            week: 1,
            roster_a: 1,
            roster_b: 2,
            p_a,
            projected_a: 100.0,
            projected_b: 100.0,
            actual_a: if a_won { 110.0 } else { 90.0 },
            actual_b: 100.0,
            sigma: 20.0,
        }
    }

    #[test]
    fn a_bigger_projection_is_favoured_but_never_certain() {
        let teams = Teams::new();
        let a: Vec<Starter> = (0..9)
            .map(|i| starter(&format!("a{i}"), "WR", 15.0))
            .collect();
        let b: Vec<Starter> = (0..9)
            .map(|i| starter(&format!("b{i}"), "WR", 12.0))
            .collect();
        let (p, sigma) = win_probability(&a, &b, &teams);
        assert!(
            p > 0.5 && p < 0.75,
            "27 points ahead of 108 is an edge, not a lock: {p}"
        );
        assert!(
            sigma > 10.0,
            "nine receivers spread more than ten points: {sigma}"
        );
    }

    #[test]
    fn a_model_that_is_always_right_scores_perfectly() {
        let games = vec![game(0.99, true), game(0.01, false), game(0.98, true)];
        let c = calibrate(&games);
        assert_eq!(c.games, 3);
        assert!((c.accuracy - 1.0).abs() < 1e-9);
        assert!(c.brier < 0.01, "brier {}", c.brier);
        assert!(c.log_loss < 0.05, "log loss {}", c.log_loss);
    }

    #[test]
    fn a_coin_flip_scores_like_one() {
        let games = vec![game(0.5, true), game(0.5, false)];
        let c = calibrate(&games);
        assert!((c.brier - 0.25).abs() < 1e-9);
        assert!((c.log_loss - 0.693).abs() < 1e-3);
        assert!((c.accuracy - 0.5).abs() < 1e-9);
    }

    #[test]
    fn buckets_hold_every_game_once() {
        let games = vec![
            game(0.2, false),
            game(0.55, true),
            game(0.8, true),
            game(0.65, false),
        ];
        let c = calibrate(&games);
        assert_eq!(c.buckets.iter().map(|b| b.games).sum::<usize>(), 4);
    }

    #[test]
    fn z_is_one_when_the_spread_is_right() {
        // Margins drawn a fixed 20 points either side of the projection with
        // sigma 20: the model's spread is exactly the realised one.
        let mut games = Vec::new();
        for i in 0..10 {
            let mut g = game(0.5, i % 2 == 0);
            g.actual_a = 100.0 + if i % 2 == 0 { 20.0 } else { -20.0 };
            g.actual_b = 100.0;
            games.push(g);
        }
        let c = calibrate(&games);
        assert!((c.z_sd - 1.0).abs() < 1e-6, "z_sd {}", c.z_sd);
        assert!(c.z_mean.abs() < 1e-9);
    }

    #[test]
    fn a_position_that_lands_on_its_projection_has_no_spread() {
        let weeks = vec![
            PlayerWeek {
                position: "QB".into(),
                projected: 20.0,
                actual: 20.0,
            },
            PlayerWeek {
                position: "QB".into(),
                projected: 18.0,
                actual: 18.0,
            },
            // Under the floor: a kicker's 2-point projection says nothing.
            PlayerWeek {
                position: "K".into(),
                projected: 2.0,
                actual: 9.0,
            },
        ];
        let fits = position_fits(&weeks);
        assert_eq!(fits.len(), 1);
        assert_eq!(fits[0].position, "QB");
        assert!(fits[0].cv < 1e-9);
        assert!((fits[0].bias - 1.0).abs() < 1e-9);
        assert!((fits[0].model_cv - position_cv("QB")).abs() < 1e-9);
    }

    #[test]
    fn the_scan_finds_a_tighter_spread_when_the_favourites_all_won() {
        // Every game a 10-point favourite, every favourite home: the season
        // wanted more confidence than a 20-point spread gave.
        let games: Vec<Game> = (0..20)
            .map(|_| Game {
                week: 1,
                roster_a: 1,
                roster_b: 2,
                p_a: 0.69,
                projected_a: 110.0,
                projected_b: 100.0,
                actual_a: 120.0,
                actual_b: 100.0,
                sigma: 20.0,
            })
            .collect();
        let (scale, loss) = best_sigma_scale(&games);
        assert!(scale < 1.0, "scale {scale}");
        assert!(loss < log_loss_at(&games, 1.0));
    }
}
