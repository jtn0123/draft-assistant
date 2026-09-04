use super::*;

fn team(roster_id: u32, wins: u32, losses: u32, points: f64, weekly: f64) -> TeamSeason {
    TeamSeason {
        roster_id,
        wins,
        losses,
        ties: 0,
        points_for: points,
        weekly_projection: (1..=4).map(|w| (w, weekly)).collect(),
        // A nine-man lineup of the same middling position: what the spread
        // model would have produced for a team projected `weekly`.
        weekly_sigma: (1..=4)
            .map(|w| (w, season_spread::team_sigma(&lineup(weekly))))
            .collect(),
    }
}

/// Nine equal starters, no stacks, adding up to `total`.
fn lineup(total: f64) -> Vec<Starter> {
    (0..9)
        .map(|_| Starter {
            position: "WR".into(),
            team: None,
            points: total / 9.0,
            uncertain: total / 9.0,
        })
        .collect()
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
fn a_tie_is_worth_half_a_win_when_the_seeds_are_cut() {
    // 1-0-1 against 1-1-0: level on wins, and the tie is the difference. The
    // simulation has always scored it that way; the seeding used to ignore it
    // and hand first seed to whoever had scored more points.
    let mut tied = team(1, 1, 0, 200.0, 100.0);
    tied.ties = 1;
    let beaten = team(2, 1, 1, 260.0, 100.0);
    let rows = standings(&[tied, beaten], &[], 1, &|id| format!("team{id}"), None, 5);
    assert_eq!(
        rows[0].roster_id, 1,
        "1-0-1 outranks 1-1 despite fewer points"
    );
    assert_eq!(rows[0].record, "1\u{2013}0\u{2013}1");
    assert_eq!(rows[0].seed, 1);
    // And the odds agree with the seeding they are printed beside.
    assert_eq!(rows[0].playoff_odds, 1.0);
    assert_eq!(rows[1].playoff_odds, 0.0);
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
    assert!((win_probability(&lineup(110.0), &lineup(110.0)) - 0.5).abs() < 1e-6);
    let favoured = win_probability(&lineup(125.0), &lineup(100.0));
    assert!(favoured > 0.5 && favoured < 1.0);
    assert!((favoured + win_probability(&lineup(100.0), &lineup(125.0)) - 1.0).abs() < 1e-6);
    // Two empty lineups are a coin flip, not a divide by zero.
    assert_eq!(win_probability(&[], &[]), 0.5);
}

#[test]
fn a_steadier_lineup_needs_a_smaller_lead_to_be_favoured() {
    // Same points on both sides of each matchup; only the spread differs.
    // The quarterback-heavy side is likelier to hold a lead than the
    // defense-heavy one is to hold the identical lead.
    let side = |position: &str, total: f64| -> Vec<Starter> {
        (0..9)
            .map(|_| Starter {
                position: position.into(),
                team: None,
                points: total / 9.0,
                uncertain: total / 9.0,
            })
            .collect()
    };
    let steady = win_probability(&side("QB", 120.0), &side("QB", 110.0));
    let wild = win_probability(&side("DEF", 120.0), &side("DEF", 110.0));
    assert!(steady > wild, "steady {steady} should beat wild {wild}");
}

#[test]
fn stacking_a_lead_makes_it_less_safe() {
    // The favourite's nine starters, all on one NFL team versus spread
    // across the league. Same projection, correlated risk.
    let side = |team: Option<&str>, total: f64| -> Vec<Starter> {
        (0..9)
            .map(|i| Starter {
                position: "WR".into(),
                team: team.map_or_else(|| Some(format!("T{i}")), |t| Some(t.to_string())),
                points: total / 9.0,
                uncertain: total / 9.0,
            })
            .collect()
    };
    let spread_out = win_probability(&side(None, 120.0), &side(None, 110.0));
    let stacked = win_probability(&side(Some("BUF"), 120.0), &side(None, 110.0));
    assert!(stacked < spread_out, "{stacked} vs {spread_out}");
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

/// The Sunday-night reading. Every one of the opponent's starters has finished
/// and I am forty points clear: the week is over in all but name, and the odds
/// have to say so. Priced off projections alone this used to sit near 80%.
#[test]
fn a_forty_point_lead_over_an_exhausted_roster_is_a_win() {
    let settled = |total: f64| -> Vec<Starter> {
        (0..9)
            .map(|_| Starter {
                position: "WR".into(),
                team: None,
                points: total / 9.0,
                uncertain: 0.0,
            })
            .collect()
    };
    // Their nine are done on 100. Mine are done on 140.
    let odds = win_probability(&settled(140.0), &settled(100.0));
    assert!(odds > 0.999, "a decided week must read as decided: {odds}");

    // And with one of mine still to play, the lead is still overwhelming.
    let mut mine = settled(140.0);
    mine.push(Starter {
        position: "WR".into(),
        team: None,
        points: 10.0,
        uncertain: 10.0,
    });
    let odds = win_probability(&mine, &settled(100.0));
    assert!(odds > 0.99, "{odds}");
}

/// The bug: from week 15 there is nothing left to simulate, so the odds
/// collapse to a flat 100%/0% that the screen printed as a forecast. The
/// percentage is unchanged — it is what the standings say — but a caller now
/// has a label to show instead of it.
#[test]
fn once_the_bracket_is_cut_there_is_a_state_to_show_instead_of_a_percentage() {
    assert_eq!(playoff_status(1, 6), "In the playoffs \u{2014} seed 1");
    assert_eq!(playoff_status(6, 6), "In the playoffs \u{2014} seed 6");
    assert_eq!(playoff_status(7, 6), "Missed the playoffs");
    // A league that somehow reports no playoff teams still cuts somebody.
    assert_eq!(playoff_status(1, 0), "In the playoffs \u{2014} seed 1");
    assert_eq!(playoff_status(2, 0), "Missed the playoffs");
}

#[test]
fn the_regular_season_leaves_the_status_unset() {
    let teams = vec![team(1, 2, 0, 210.0, 100.0), team(2, 1, 1, 260.0, 100.0)];
    let rows = standings(&teams, &[], 1, &|id| format!("team{id}"), Some(1), 5);
    assert!(
        rows.iter().all(|r| r.playoff_status.is_none()),
        "only the caller that knows the week may fill this in"
    );
}
