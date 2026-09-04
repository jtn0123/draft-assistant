use super::*;
use crate::scoring::norm_cdf;

fn starter(position: &str, team: Option<&str>, points: f64) -> Starter {
    Starter {
        position: position.into(),
        team: team.map(str::to_string),
        points,
        uncertain: points,
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
    let a: Vec<Starter> = (0..9).map(|_| starter("WR", None, 15.0)).collect();
    let b: Vec<Starter> = (0..9).map(|_| starter("WR", None, 12.0)).collect();
    let (p, sigma) = win_probability(&a, &b);
    assert!(
        p > 0.5 && p < 0.75,
        "27 points ahead of 108 is an edge, not a lock: {p}"
    );
    assert!(
        sigma > 10.0,
        "nine receivers spread more than ten points: {sigma}"
    );
}

/// The tables read `sigma` as the spread that produced `p_a` — `z_sd` and the
/// whole `best_sigma_scale` scan are meaningless otherwise. `p` comes from the
/// shipped `season_odds::win_probability` and `sigma` is recomputed here, so
/// this is the seam that has to hold.
#[test]
fn the_reported_sigma_is_the_one_behind_the_probability() {
    let a = [
        starter("QB", Some("BUF"), 22.0),
        starter("WR", Some("BUF"), 16.0),
        starter("RB", Some("KC"), 14.0),
        starter("DEF", None, 8.0),
    ];
    let b = [
        starter("QB", Some("PHI"), 19.0),
        starter("TE", None, 11.0),
        starter("WR", Some("SF"), 13.0),
        starter("K", None, 8.0),
    ];
    let (p, sigma) = win_probability(&a, &b);
    let margin: f64 =
        a.iter().map(|s| s.points).sum::<f64>() - b.iter().map(|s| s.points).sum::<f64>();
    assert!(
        (p - norm_cdf(margin / sigma)).abs() < 1e-12,
        "{p} vs sigma {sigma}"
    );
    // A stack widens the spread, so it must widen this one too.
    let apart = [
        starter("QB", Some("BUF"), 22.0),
        starter("WR", Some("MIA"), 16.0),
        starter("RB", Some("KC"), 14.0),
        starter("DEF", None, 8.0),
    ];
    assert!(win_probability(&apart, &b).1 < sigma);
}

/// Two sides that project to nothing are a coin flip, and the spread is still
/// a real number rather than a zero that would divide `Game::z` by nothing.
#[test]
fn empty_lineups_are_a_coin_flip_with_a_usable_spread() {
    let (p, sigma) = win_probability(&[], &[]);
    assert!((p - 0.5).abs() < 1e-12);
    assert!(sigma > 0.0, "sigma {sigma}");
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
fn no_games_is_an_empty_report_rather_than_a_division_by_zero() {
    let c = calibrate(&[]);
    assert_eq!(c.games, 0);
    assert!(c.buckets.is_empty());
    assert_eq!(log_loss_at(&[], 1.0), 0.0);
    assert_eq!(best_sigma_scale(&[]), (1.0, 0.0));
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
