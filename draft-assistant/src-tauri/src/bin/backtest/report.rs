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

#[cfg(test)]
mod tests {
    use super::*;

    fn game(week: u32, p_a: f64, a: f64, b: f64) -> Game {
        Game {
            week,
            roster_a: 1,
            roster_b: 2,
            p_a,
            projected_a: 110.0,
            projected_b: 100.0,
            actual_a: a,
            actual_b: b,
            sigma: 25.0,
        }
    }

    fn season() -> Vec<Game> {
        (1..=8)
            .map(|week| {
                let favourite_won = week % 3 != 0;
                game(week, 0.65, if favourite_won { 120.0 } else { 90.0 }, 100.0)
            })
            .collect()
    }

    /// The JSON handed to `--out` is the printed table's own figures plus the
    /// two the fit adds. A report that printed one thing and wrote another
    /// would make every saved run unreadable against the console it came from.
    #[test]
    fn the_json_a_report_returns_carries_the_figures_it_printed() {
        let value = report("under test", &season());
        let map = value.as_object().expect("an object");
        assert_eq!(map["games"], serde_json::json!(8));
        for key in ["accuracy", "brier", "log_loss", "buckets"] {
            assert!(map.contains_key(key), "no {key} in {map:?}");
        }
        assert!(map["best_sigma_scale"].as_f64().expect("a scale") > 0.0);
        assert!(map["log_loss_at_best_scale"].as_f64().is_some());
    }

    /// A run with no games at all — an unplayed season, or a league whose
    /// weeks all came back empty — used to be where the summary panicked on
    /// an empty slice.
    #[test]
    fn a_season_with_no_games_reports_rather_than_panicking() {
        let value = report("empty", &[]);
        assert_eq!(value["games"], serde_json::json!(0));
        holdout(&[]);
        assert!(positions(&[]).is_empty());
    }

    /// The holdout needs a fit half and a test half. One week is all fit and
    /// no test, and printing a "grade" off nothing would be worse than
    /// printing none.
    #[test]
    fn a_holdout_with_nothing_left_to_grade_says_nothing() {
        holdout(&[game(1, 0.6, 120.0, 100.0)]);
        holdout(&season());
    }

    #[test]
    fn a_position_fit_is_reported_for_every_position_with_startable_weeks() {
        let weeks: Vec<PlayerWeek> = ["RB", "WR"]
            .iter()
            .flat_map(|position| {
                (0..6).map(move |n| PlayerWeek {
                    position: position.to_string(),
                    projected: 12.0,
                    actual: 10.0 + f64::from(n),
                })
            })
            .collect();
        let fits = positions(&weeks);
        let named: Vec<&str> = fits.iter().map(|f| f.position.as_str()).collect();
        assert!(named.contains(&"RB") && named.contains(&"WR"), "{named:?}");
        assert!(fits.iter().all(|f| f.games == 6));
    }

    #[test]
    fn a_written_report_reads_back_as_the_shape_the_notebook_expects() {
        let dir = std::env::temp_dir().join(format!("backtest-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("calib.json");
        let games = season();
        write_json(
            &path.to_string_lossy(),
            "1389710366300200960",
            report("as set", &games),
            report("as best", &games),
            &positions(&[]),
        );
        let text = std::fs::read_to_string(&path).expect("the report was written");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["league_id"], "1389710366300200960");
        assert_eq!(value["as_set"]["games"], serde_json::json!(8));
        assert!(value["as_best"].is_object());
        assert!(value["positions"].is_array());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A `--out` pointing somewhere unwritable is a warning on stderr, never
    /// a panic: the run's own tables have already been printed and are the
    /// thing the user was waiting for.
    #[test]
    fn a_report_that_cannot_be_written_does_not_take_the_run_down_with_it() {
        write_json(
            "/nonexistent-directory/calib.json",
            "123",
            serde_json::Value::Null,
            serde_json::Value::Null,
            &[],
        );
    }
}
