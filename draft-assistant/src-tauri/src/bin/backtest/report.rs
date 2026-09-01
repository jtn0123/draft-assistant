//! Printing. Every number here comes out of `draft_assistant_lib::backtest`;
//! this module only decides how it reads.

use draft_assistant_lib::backtest::{
    best_sigma_scale, calibrate, log_loss_at, position_fits, Game, PlayerWeek, PositionFit,
};

/// One calibration table, printed, and the same figures as JSON.
pub fn report(label: &str, games: &[Game]) -> serde_json::Value {
    let c = calibrate(games);
    println!("\n{label}: {} games", c.games);
    println!(
        "  favourite won {:.0}% · brier {:.3} (coin flip .250) · log loss {:.3} (.693)",
        c.accuracy * 100.0,
        c.brier,
        c.log_loss
    );
    println!(
        "  margin error {:.2} of the spread (1.00 = a normal season; under 1 \
         is the hedge for upsets) · bias {:+.2} · score error {:.1} pts",
        c.z_sd, c.z_mean, c.score_mae
    );
    println!("  band          games   said   won");
    for b in &c.buckets {
        println!(
            "  {:.0}–{:.0}%{:>12}  {:5.0}% {:5.0}%",
            b.low * 100.0,
            b.high * 100.0,
            b.games,
            b.predicted * 100.0,
            b.actual * 100.0
        );
    }
    let (scale, loss) = best_sigma_scale(games);
    println!(
        "  best spread scale {:.2} → log loss {:.3} (from {:.3} at 1.00)",
        scale,
        loss,
        log_loss_at(games, 1.0)
    );
    let mut value = serde_json::to_value(&c).unwrap_or(serde_json::Value::Null);
    if let Some(map) = value.as_object_mut() {
        map.insert("best_sigma_scale".into(), scale.into());
        map.insert("log_loss_at_best_scale".into(), loss.into());
    }
    value
}

/// Fit the spread scale on the first half of the season and grade it on the
/// second: a scale that only helps the games it was fitted to is noise.
pub fn holdout(games: &[Game]) {
    let Some(last) = games.iter().map(|g| g.week).max() else {
        return;
    };
    let cut = last / 2;
    let (fit, test): (Vec<Game>, Vec<Game>) = games.iter().cloned().partition(|g| g.week <= cut);
    if fit.is_empty() || test.is_empty() {
        return;
    }
    let (scale, _) = best_sigma_scale(&fit);
    println!(
        "\nholdout: scale {:.2} fitted on weeks 1–{cut} ({} games) → weeks {}–{last} ({} games) \
         log loss {:.3}, against {:.3} unscaled",
        scale,
        fit.len(),
        cut + 1,
        test.len(),
        log_loss_at(&test, scale),
        log_loss_at(&test, 1.0)
    );
}

/// The per-position spread fit — where `position_cv` came from.
pub fn positions(weeks: &[PlayerWeek]) -> Vec<PositionFit> {
    println!("\nper position, starters projected 4+ points");
    println!("  pos   weeks   bias    real cv   model cv");
    let fits = position_fits(weeks);
    for f in &fits {
        println!(
            "  {:<5} {:>6}   {:.2}     {:.2}      {:.2}",
            f.position, f.games, f.bias, f.cv, f.model_cv
        );
    }
    fits
}

pub fn write_json(
    path: &str,
    league_id: &str,
    as_set: serde_json::Value,
    as_best: serde_json::Value,
    fits: &[PositionFit],
) {
    let value = serde_json::json!({
        "league_id": league_id,
        "as_set": as_set,
        "as_best": as_best,
        "positions": fits,
    });
    match serde_json::to_string_pretty(&value) {
        Ok(text) => {
            if let Err(error) = std::fs::write(path, text) {
                eprintln!("could not write {path}: {error}");
            }
        }
        Err(error) => eprintln!("could not encode report: {error}"),
    }
}
