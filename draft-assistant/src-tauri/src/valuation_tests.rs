use super::*;

fn sp(pos: &str, pts: f64) -> ScoredPlayer {
    ScoredPlayer {
        position: pos.into(),
        points: pts,
    }
}

#[test]
fn flex_demand_goes_to_best_positions() {
    // 2 teams, 1 RB + 1 WR + 1 FLEX each. RBs dominate the overflow, so
    // flex demand should land on RB.
    let roster: Vec<String> = ["RB", "WR", "FLEX", "BN"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let players = vec![
        sp("RB", 300.0),
        sp("RB", 290.0),
        sp("RB", 280.0),
        sp("RB", 270.0),
        sp("WR", 250.0),
        sp("WR", 240.0),
        sp("WR", 100.0),
        sp("WR", 90.0),
    ];
    let model = compute_replacement(&players, &RosterRules::new(&roster), 2, None);
    assert_eq!(model.demand.get("RB"), Some(&4)); // 2 dedicated + 2 flex
    assert_eq!(model.demand.get("WR"), Some(&2)); // dedicated only
}

#[test]
fn mixed_flex_types_allocate_only_eligible_players() {
    let players = vec![
        sp("QB", 300.0),
        sp("QB", 290.0),
        sp("QB", 280.0),
        sp("WR", 250.0),
        sp("WR", 240.0),
        sp("WR", 230.0),
        sp("TE", 220.0),
        sp("TE", 210.0),
        sp("RB", 200.0),
        sp("RB", 190.0),
    ];
    let slots = ["QB", "SUPER_FLEX", "REC_FLEX"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect::<Vec<_>>();
    let model = compute_replacement(&players, &RosterRules::new(&slots), 1, None);

    assert_eq!(model.demand.get("QB"), Some(&2));
    assert_eq!(model.demand.get("WR"), Some(&1));
}

#[test]
fn tiers_break_on_gaps() {
    let pts = vec![300.0, 295.0, 250.0, 248.0, 246.0, 200.0];
    let tiers = assign_tiers(&pts, 20.0);
    assert_eq!(tiers, vec![1, 1, 2, 2, 2, 3]);
}

/// A flex league whose WR pool is deep and flat and whose RB pool falls off a
/// cliff a few players past the starters — the shape real projections have,
/// exaggerated so the two allocators disagree loudly.
///
/// Every WR is worth exactly the same, and each of the three RBs just past the
/// dedicated starters is worth a point *less* than a WR. On raw points the WR
/// therefore wins all eight flex slots forever, which is the bug. On marginal
/// value those three RBs are the last ones before the cliff and the WR behind
/// the WR is worth nothing extra, so the RBs go first.
fn cliff_league() -> (Vec<ScoredPlayer>, RosterRules) {
    let mut players = Vec::new();
    // 8 dedicated RB starters, then the three either side of the cliff, then
    // a long tail of replacement-level bodies.
    for i in 0..8 {
        players.push(sp("RB", 260.0 - 5.0 * f64::from(i)));
    }
    players.push(sp("RB", 209.0));
    players.push(sp("RB", 208.0));
    players.push(sp("RB", 207.0));
    for _ in 0..12 {
        players.push(sp("RB", 100.0));
    }
    for _ in 0..40 {
        players.push(sp("WR", 210.0));
    }
    let slots: Vec<String> = ["RB", "RB", "WR", "WR", "FLEX", "FLEX", "BN"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect();
    (players, RosterRules::new(&slots))
}

#[test]
fn raw_points_hand_every_flex_slot_to_the_flat_pool() {
    // flex_bias 0.0 is the old allocator, kept reachable so the regression it
    // caused stays legible: WR takes all 8 flex slots and RB never moves off
    // its dedicated demand.
    let (players, rules) = cliff_league();
    let model = compute_replacement(&players, &rules, 4, Some(0.0));
    assert_eq!(model.demand.get("RB"), Some(&8));
    assert_eq!(model.demand.get("WR"), Some(&16));
}

#[test]
fn marginal_value_gives_the_steep_pool_its_share() {
    let (players, rules) = cliff_league();
    let model = compute_replacement(&players, &rules, 4, None);
    // The three RBs above the cliff are worth more than another interchangeable
    // WR, so they take the first three flex slots; the fourth RB is over the
    // cliff and the remaining five slots go to WR.
    assert_eq!(model.demand.get("RB"), Some(&11));
    assert_eq!(model.demand.get("WR"), Some(&13));
    // And the baseline follows the demand down the cliff.
    assert!(model.baseline["RB"] < 210.0);
}

#[test]
fn flex_bias_is_the_only_thing_between_the_two_answers() {
    // Same league, same pools: the knob alone decides whether the steep pool
    // is seen at all. Turning it up never costs that pool demand.
    let (players, rules) = cliff_league();
    let mut previous = 0usize;
    for bias in [0.0, 0.25, 1.0, 2.0] {
        let rb = compute_replacement(&players, &rules, 4, Some(bias)).demand["RB"];
        assert!(
            rb >= previous,
            "flex_bias {bias} gave RB {rb}, fewer than the {previous} a lighter bias did"
        );
        previous = rb;
    }
    assert!(previous > compute_replacement(&players, &rules, 4, Some(0.0)).demand["RB"]);
}

#[test]
fn a_pool_that_runs_out_stops_taking_flex_slots() {
    // Only two WRs exist; the flex slot past them cannot go to WR, and looking
    // one round deeper must not run off the end of the pool.
    let players = vec![
        sp("WR", 300.0),
        sp("WR", 290.0),
        sp("RB", 200.0),
        sp("RB", 190.0),
        sp("RB", 180.0),
        sp("RB", 170.0),
        sp("RB", 160.0),
    ];
    let slots: Vec<String> = ["WR", "FLEX", "FLEX"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect();
    let model = compute_replacement(&players, &RosterRules::new(&slots), 1, None);
    assert_eq!(model.demand.get("WR"), Some(&2));
    assert_eq!(model.demand.get("RB"), Some(&1));
}
