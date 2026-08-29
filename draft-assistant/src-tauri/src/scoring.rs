//! Custom scoring engine.
//!
//! The league's `scoring_settings` map and Sleeper's projected stat lines share
//! the same key space (pass_yd, rush_td, rec, sack, pts_allow_7_13, ...), so
//! base scoring is a dot product — scoring rules stay data, exactly as the
//! spec demands. The one thing a season-total dot product cannot express is
//! per-game yardage bonuses (bonus_pass_yd_300, bonus_rush_yd_100, ...);
//! those are modeled from weekly per-game projections with a lognormal-ish
//! game distribution and added as an expected-points term.

use std::collections::HashMap;

/// Stat keys that are metadata riding along in the projections payload, never
/// scorable stats. NOTE: `pts_allow_*` (DEF points-allowed bucket game counts)
/// IS scorable, so the fantasy-point totals are matched exactly, not by prefix.
const NON_STAT_KEYS: [&str; 4] = ["pts_ppr", "pts_half_ppr", "pts_std", "gp"];
const NON_STAT_PREFIXES: [&str; 2] = ["adp_", "pos_adp"];

/// Per-game yardage-bonus scoring keys and the projection stat they threshold.
/// (scoring_key, stat_key, lower_bound_inclusive, upper_bound_exclusive)
const GAME_BONUSES: [(&str, &str, f64, f64); 6] = [
    ("bonus_rush_yd_100", "rush_yd", 100.0, 200.0),
    ("bonus_rush_yd_200", "rush_yd", 200.0, f64::INFINITY),
    ("bonus_rec_yd_100", "rec_yd", 100.0, 200.0),
    ("bonus_rec_yd_200", "rec_yd", 200.0, f64::INFINITY),
    ("bonus_pass_yd_300", "pass_yd", 300.0, 400.0),
    ("bonus_pass_yd_400", "pass_yd", 400.0, f64::INFINITY),
];

/// Game-level volatility (sigma / mean) per stat. Passing volume is much more
/// stable week to week than rushing/receiving yardage.
fn game_cv(stat_key: &str) -> f64 {
    match stat_key {
        "pass_yd" => 0.30,
        "rush_yd" => 0.55,
        "rec_yd" => 0.60,
        _ => 0.50,
    }
}

/// Standard normal CDF via the Abramowitz & Stegun erf approximation.
pub fn norm_cdf(z: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * z.abs());
    let d = 0.3989423 * (-z * z / 2.0).exp();
    let p =
        d * t * (0.3193815 + t * (-0.3565638 + t * (1.781478 + t * (-1.821256 + t * 1.330274))));
    if z > 0.0 {
        1.0 - p
    } else {
        p
    }
}

/// Upper tail P(Z > z), computed directly so small tails do not cancel to 0.
pub fn norm_sf(z: f64) -> f64 {
    if z > 0.0 {
        norm_cdf(-z)
    } else {
        1.0 - norm_cdf(z)
    }
}

/// P(lo <= Y < hi) for a game where Y ~ Normal(mean, cv*mean).
fn band_probability(mean: f64, lo: f64, hi: f64, cv: f64) -> f64 {
    // Below this a bonus game is effectively impossible and the normal
    // approximation is garbage anyway.
    if mean < lo * 0.25 {
        return 0.0;
    }
    let sigma = (cv * mean).max(1.0);
    let p_hi = if hi.is_finite() {
        norm_cdf((hi - mean) / sigma)
    } else {
        1.0
    };
    let p_lo = norm_cdf((lo - mean) / sigma);
    (p_hi - p_lo).max(0.0)
}

/// Season base points: dot product of projected season stats with the league
/// scoring settings. ADP/pts metadata keys are excluded; any scoring key with
/// no matching stat contributes nothing (and vice versa).
pub fn base_points(stats: &HashMap<String, f64>, scoring: &HashMap<String, f64>) -> f64 {
    stats
        .iter()
        .filter(|(k, _)| !NON_STAT_KEYS.contains(&k.as_str()))
        .filter(|(k, _)| !NON_STAT_PREFIXES.iter().any(|p| k.starts_with(p)))
        // Game bonuses are handled by the expectation model, never dot product.
        .filter(|(k, _)| {
            !k.starts_with("bonus_pass_yd")
                && !k.starts_with("bonus_rush_yd")
                && !k.starts_with("bonus_rec_yd")
        })
        .filter_map(|(k, v)| scoring.get(k).map(|w| w * v))
        .sum()
}

/// Expected season points from per-game yardage bonuses, computed from weekly
/// per-game projected means. `weekly_stats` is one entry per projected game.
pub fn bonus_points(weekly_stats: &[&HashMap<String, f64>], scoring: &HashMap<String, f64>) -> f64 {
    let mut total = 0.0;
    for (score_key, stat_key, lo, hi) in GAME_BONUSES {
        let Some(&pts) = scoring.get(score_key) else {
            continue;
        };
        if pts == 0.0 {
            continue;
        }
        let cv = game_cv(stat_key);
        for wk in weekly_stats {
            if let Some(&mean) = wk.get(stat_key) {
                total += pts * band_probability(mean, lo, hi, cv);
            }
        }
    }
    total
}

/// Each player's projected points for each week, scored the way the season
/// total is: the stat line under league scoring plus the expected per-game
/// yardage bonuses for that one game.
pub fn weekly_points_by_player(
    rows: &[crate::sleeper::ProjectionRow],
    scoring: &HashMap<String, f64>,
) -> HashMap<String, Vec<(u32, f64)>> {
    let mut out: HashMap<String, Vec<(u32, f64)>> = HashMap::new();
    for row in rows {
        let (Some(week), Some(stats)) = (row.week, row.stats.as_ref()) else {
            continue;
        };
        let pts = base_points(stats, scoring) + bonus_points(&[stats], scoring);
        out.entry(row.player_id.clone())
            .or_default()
            .push((week, pts));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoring_fixture() -> HashMap<String, f64> {
        HashMap::from([
            ("pass_yd".into(), 0.04),
            ("pass_td".into(), 6.0),
            ("pass_int".into(), -2.0),
            ("rec".into(), 1.0),
            ("rec_yd".into(), 0.1),
            ("rush_yd".into(), 0.1),
            ("bonus_rush_yd_100".into(), 3.0),
            ("bonus_rush_yd_200".into(), 4.0),
        ])
    }

    #[test]
    fn dot_product_ignores_metadata_keys() {
        let stats = HashMap::from([
            ("pass_yd".into(), 4000.0),
            ("pass_td".into(), 30.0),
            ("pass_int".into(), 10.0),
            ("adp_ppr".into(), 1.5),
            ("pts_ppr".into(), 999.0),
            ("gp".into(), 17.0),
        ]);
        let pts = base_points(&stats, &scoring_fixture());
        assert!((pts - (160.0 + 180.0 - 20.0)).abs() < 1e-9);
    }

    #[test]
    fn bonus_probability_sane() {
        // A 120-yd/game projected back should have a strong 100+ chance,
        // near-zero 200+ chance; a 30-yd/game player near zero for both.
        let big: HashMap<String, f64> = HashMap::from([("rush_yd".into(), 120.0)]);
        let small: HashMap<String, f64> = HashMap::from([("rush_yd".into(), 30.0)]);
        let scoring = scoring_fixture();
        let b_big = bonus_points(&[&big], &scoring);
        let b_small = bonus_points(&[&small], &scoring);
        assert!(
            b_big > 1.0,
            "expected >1 bonus pt/game for 120yd mean, got {b_big}"
        );
        assert!(b_small < 0.2, "expected ~0 for 30yd mean, got {b_small}");
        assert!(b_big < 4.0, "cannot exceed max bonus, got {b_big}");
    }

    #[test]
    fn norm_cdf_is_symmetric() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((norm_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((norm_cdf(-1.96) - 0.025).abs() < 1e-3);
    }
}
