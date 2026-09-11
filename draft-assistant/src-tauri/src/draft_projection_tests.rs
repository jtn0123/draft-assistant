//! What the draft adds up to, checked on rosters small enough to add up by
//! hand.

use super::*;

fn rules() -> RosterRules {
    RosterRules::new(&[
        "QB".to_string(),
        "RB".to_string(),
        "RB".to_string(),
        "WR".to_string(),
        "FLEX".to_string(),
        "BN".to_string(),
        "BN".to_string(),
    ])
}

fn player<'a>(id: &'a str, position: &'a str, points: f64) -> Drafted<'a> {
    Drafted {
        player_id: id,
        position,
        team: Some("SF"),
        points: Some(points),
    }
}

fn team<'a>(slot: u32, players: Vec<Drafted<'a>>) -> Team<'a> {
    Team {
        slot,
        name: Some(format!("Team {slot}")),
        is_mine: slot == 1,
        players,
    }
}

#[test]
fn the_best_lineup_fills_fixed_slots_before_flex() {
    // Three backs: the two best start at RB, and the FLEX takes the third
    // rather than the best one, which would leave RB2 to a worse player.
    let players = vec![
        player("qb", "QB", 300.0),
        player("rb1", "RB", 250.0),
        player("rb2", "RB", 200.0),
        player("rb3", "RB", 150.0),
        player("wr1", "WR", 220.0),
        player("wr2", "WR", 100.0),
    ];
    let rows = project(&rules(), &[team(1, players)]);

    // QB 300 + RB 250 + RB 200 + WR 220 + FLEX 150 = 1120, and WR2 benched.
    assert_eq!(rows[0].starters, 1120.0);
    assert_eq!(rows[0].bench, 100.0);
    assert!(rows[0].holes.is_empty(), "{:?}", rows[0].holes);
}

#[test]
fn a_slot_nobody_can_fill_is_named_rather_than_scored_as_zero() {
    // No quarterback drafted: the lineup is short one, and says which one.
    let rows = project(
        &rules(),
        &[team(
            1,
            vec![
                player("rb1", "RB", 250.0),
                player("rb2", "RB", 200.0),
                player("wr1", "WR", 220.0),
                player("wr2", "WR", 180.0),
            ],
        )],
    );

    assert_eq!(rows[0].holes, vec!["QB".to_string()]);
    assert_eq!(rows[0].starters, 850.0);
}

/// A player the board has no projection for cannot be scored, so he is not
/// started: a zero in a starting slot would flatter the roster that drafted
/// him over one that left the slot open, which is the same roster.
#[test]
fn a_player_with_no_projection_starts_nowhere() {
    let mut players = vec![player("rb1", "RB", 250.0), player("rb2", "RB", 200.0)];
    players.push(Drafted {
        player_id: "rookie",
        position: "QB",
        team: None,
        points: None,
    });
    let rows = project(&rules(), &[team(1, players)]);

    // Two backs start; the quarterback cannot be scored, so QB, WR and the
    // FLEX behind them are all still open.
    assert_eq!(
        rows[0].holes,
        vec!["QB".to_string(), "WR".to_string(), "FLEX".to_string()]
    );
    assert_eq!(rows[0].starters, 450.0);
    assert_eq!(rows[0].bench, 0.0);
}

#[test]
fn rosters_come_back_best_first_with_the_odds_they_win_on_points() {
    let strong = team(
        1,
        vec![
            player("a", "QB", 320.0),
            player("b", "RB", 280.0),
            player("c", "RB", 260.0),
            player("d", "WR", 250.0),
            player("e", "WR", 240.0),
        ],
    );
    let weak = team(
        2,
        vec![
            player("f", "QB", 200.0),
            player("g", "RB", 150.0),
            player("h", "RB", 140.0),
            player("i", "WR", 130.0),
            player("j", "WR", 120.0),
        ],
    );
    let rows = project(&rules(), &[weak, strong]);

    assert_eq!(rows[0].slot, 1, "the better roster is first");
    assert_eq!(rows[0].rank, 1);
    assert_eq!(rows[1].rank, 2);
    assert!(rows[0].is_mine);
    // Six hundred points clear over a season is not a coin flip, and is not a
    // certainty either.
    assert!(
        rows[0].title_odds.unwrap() > 0.9,
        "{:?}",
        rows[0].title_odds
    );
    assert!(
        rows[1].title_odds.unwrap() < 0.1,
        "{:?}",
        rows[1].title_odds
    );
    let total: f64 = rows.iter().map(|row| row.title_odds.unwrap()).sum();
    assert!(
        (total - 1.0).abs() < 1e-9,
        "the odds are a share of one: {total}"
    );
}

/// Two rosters that project the same are a coin flip, and the same board
/// gives the same answer every time it is asked.
#[test]
fn equal_rosters_split_the_odds_and_the_answer_does_not_wander() {
    let squad = |slot: u32| {
        team(
            slot,
            vec![
                player("a", "QB", 300.0),
                player("b", "RB", 250.0),
                player("c", "RB", 200.0),
                player("d", "WR", 220.0),
            ],
        )
    };
    let once = project(&rules(), &[squad(1), squad(2)]);
    let twice = project(&rules(), &[squad(1), squad(2)]);

    assert_eq!(once, twice);
    assert!(
        (once[0].title_odds.unwrap() - 0.5).abs() < 0.05,
        "{:?}",
        once[0].title_odds
    );
}

#[test]
fn an_empty_draft_projects_nothing_rather_than_dividing_by_zero() {
    let rows = project(&rules(), &[team(1, Vec::new())]);

    assert_eq!(rows[0].starters, 0.0);
    assert_eq!(
        rows[0].title_odds, None,
        "empty rosters have no useful odds"
    );
    assert_eq!(rows[0].holes.len(), 5);
}

#[test]
fn restrictive_flex_slots_fill_before_superflex_regardless_of_order() {
    for slots in [["SUPER_FLEX", "FLEX"], ["FLEX", "SUPER_FLEX"]] {
        let rules = RosterRules::new(&slots.map(str::to_string));
        let rows = project(
            &rules,
            &[team(
                1,
                vec![
                    player("qb", "QB", 95.0),
                    player("rb", "RB", 100.0),
                    player("wr", "WR", 90.0),
                ],
            )],
        );
        assert_eq!(rows[0].starters, 195.0, "{slots:?}");
        assert_eq!(rows[0].bench, 90.0);
        assert!(rows[0].holes.is_empty());
    }
}

#[test]
fn exact_simulation_ties_share_credit_without_a_seat_advantage() {
    assert_eq!(
        title_odds(&[10.0, 10.0, 5.0], &[0.0; 3]),
        vec![0.5, 0.5, 0.0]
    );
}

#[test]
fn multiple_empty_rosters_have_no_odds_until_projections_exist() {
    let rows = project(&rules(), &[team(1, vec![]), team(2, vec![])]);
    let json = serde_json::to_value(rows).unwrap();
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["title_odds"].is_null()));
}

#[test]
fn all_missing_player_projections_cannot_make_a_favorite() {
    let teams: Vec<_> = (1..=3)
        .map(|slot| {
            team(
                slot,
                vec![Drafted {
                    player_id: "unpriced",
                    position: "QB",
                    team: None,
                    points: None,
                }],
            )
        })
        .collect();
    let json = serde_json::to_value(project(&rules(), &teams)).unwrap();
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["title_odds"].is_null()));
}
