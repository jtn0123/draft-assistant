//! What the league's own rules do to the score.
//!
//! Every number in here used to be a constant fitted to one league — twelve
//! teams, one quarterback, no keepers — and quietly wrong in any other. These
//! are the cases where the league disagrees with that default.

use super::score::{score_candidate, Context};
use super::tests::{entry, of_mode, player, recs, slots};
use super::*;
use crate::board::AvailablePlayer;

fn rules(slots: &[&str]) -> RosterRules {
    RosterRules::new(
        &slots
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>(),
    )
}

fn superflex_slots() -> Vec<&'static str> {
    vec![
        "QB",
        "RB",
        "RB",
        "WR",
        "WR",
        "TE",
        "FLEX",
        "SUPER_FLEX",
        "BN",
        "BN",
    ]
}

/// A roster with the open starting slots the engine would report for it.
fn roster_with(positions: &[&str], open: &[(&str, u32)]) -> TeamRoster {
    TeamRoster {
        slot: 2,
        display_name: None,
        players: positions
            .iter()
            .enumerate()
            .map(|(i, p)| entry(p, i as u32 + 1))
            .collect(),
        open_starters: open
            .iter()
            .map(|(slot, n)| ((*slot).to_string(), *n))
            .collect(),
    }
}

fn reasons_for<'a>(recs: &'a [Recommendation], id: &str) -> Option<&'a Vec<String>> {
    recs.iter().find(|r| r.player_id == id).map(|r| &r.reasons)
}

// ---------- superflex ----------

#[test]
fn a_superflex_league_wants_a_second_quarterback_and_does_not_dock_him() {
    // Round 5 of a twelve-team superflex draft, one quarterback rostered. The
    // SUPER_FLEX slot is the single biggest hole on this roster, and the
    // discipline layer used to read the second QB as a backup and take 25
    // points off him for it — for filling a starting slot.
    let available = vec![player("qb2", "QB", 60.0)];
    let mine = roster_with(
        &["QB", "RB", "WR", "TE"],
        &[("RB", 1), ("WR", 1), ("FLEX", 1), ("SUPER_FLEX", 1)],
    );
    let have: HashMap<&str, u32> = HashMap::from([("QB", 1), ("RB", 1), ("WR", 1), ("TE", 1)]);

    let superflex = rules(&superflex_slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &superflex, 5, 15, 50, 12);
    let scored = score_candidate(
        &context(&inputs, have.clone()),
        &available[0],
        Mode::Balanced,
    )
    .expect("a second QB in superflex is not disqualified");
    let total = scored.total;
    let reasons = scored.into_reasons();
    assert!(
        !reasons.iter().any(|r| r.contains("backup QB")),
        "{reasons:?}"
    );
    assert!(
        reasons.iter().any(|r| r.contains("SUPER_FLEX")),
        "{reasons:?}"
    );

    // The same roster and the same board in a one-quarterback league: there he
    // really is a backup, and the two scores have to be a long way apart.
    let standard = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &standard, 5, 15, 50, 12);
    let backup = score_candidate(&context(&inputs, have), &available[0], Mode::Balanced)
        .expect("a second QB in a one-QB league is depth, not disqualified");
    assert!(
        total - backup.total > 20.0,
        "superflex {total} vs one-QB {}",
        backup.total
    );
}

#[test]
fn a_third_quarterback_in_superflex_is_depth_not_a_disqualification() {
    // Two starters are filled, so the third is a bench body — priced as one,
    // not refused outright the way the one-QB cap refused him.
    let available = vec![player("qb3", "QB", 200.0), player("wr9", "WR", 5.0)];
    let mine = roster_with(&["QB", "QB", "RB", "WR"], &[("FLEX", 1)]);
    let deep = recs(
        &available,
        Some(&mine),
        &rules(&superflex_slots()),
        8,
        15,
        90,
    );
    let reasons = reasons_for(&deep, "qb3")
        .unwrap_or_else(|| panic!("a 200-VORP third QB was disqualified: {deep:?}"));
    assert!(
        reasons.iter().any(|r| r.contains("backup QB")),
        "{reasons:?}"
    );
    // A fourth is still off the board — one spare beyond what the league starts.
    let mine = roster_with(&["QB", "QB", "QB", "RB"], &[("FLEX", 1)]);
    let fourth = recs(
        &available,
        Some(&mine),
        &rules(&superflex_slots()),
        8,
        15,
        90,
    );
    assert!(fourth.iter().all(|r| r.position != "QB"), "{fourth:?}");
}

#[test]
fn a_one_quarterback_league_still_docks_the_backup_and_refuses_the_third() {
    let available = vec![player("qb2", "QB", 60.0), player("wr3", "WR", 55.0)];
    let mine = roster_with(&["QB", "RB", "WR", "TE"], &[("FLEX", 2)]);
    let standard = RosterRules::new(&slots());
    let backup = recs(&available, Some(&mine), &standard, 5, 15, 50);
    if let Some(reasons) = reasons_for(&backup, "qb2") {
        assert!(
            reasons.iter().any(|r| r.contains("backup QB")),
            "{reasons:?}"
        );
    }
    let mine = roster_with(&["QB", "QB", "RB", "WR"], &[("FLEX", 2)]);
    let third = recs(&available, Some(&mine), &standard, 5, 15, 50);
    assert!(third.iter().all(|r| r.position != "QB"), "{third:?}");
}

#[test]
fn two_tight_end_leagues_get_the_same_treatment() {
    let two_te = rules(&["QB", "RB", "WR", "TE", "TE", "FLEX", "BN", "BN"]);
    let available = vec![player("te2", "TE", 70.0), player("wr3", "WR", 20.0)];
    let mine = roster_with(&["QB", "RB", "WR", "TE"], &[("TE", 1), ("FLEX", 1)]);
    let recs = recs(&available, Some(&mine), &two_te, 6, 15, 60);
    let reasons = reasons_for(&recs, "te2")
        .unwrap_or_else(|| panic!("the second TE of two starters was refused: {recs:?}"));
    assert!(
        !reasons.iter().any(|r| r.contains("backup TE")),
        "{reasons:?}"
    );
}

// ---------- keepers ----------

#[test]
fn keepers_ahead_of_the_pick_do_not_make_everybody_look_like_a_bargain() {
    // Twenty-six keepers are in the book before pick 30, so pick 30 is the
    // fourth name actually called. An ADP-12 player has not fallen anywhere;
    // read at the board's own pick number he looked eighteen picks past ADP.
    let mut available = vec![player("wr1", "WR", 40.0), player("rb1", "RB", 38.0)];
    available[0].player.adp = Some(12.0);
    available[1].player.adp = Some(12.0);
    let mine = roster_with(&["RB", "WR"], &[("QB", 1), ("TE", 1), ("FLEX", 2)]);
    let standard = RosterRules::new(&slots());

    let mut inputs = RecommendInputs::new(&available, Some(&mine), &standard, 3, 15, 30, 12);
    inputs.market_pick = 4; // 26 keepers ahead of pick 30
    let kept = recommend(&inputs);
    assert!(
        of_mode(&kept, "balanced")
            .reasons
            .iter()
            .all(|r| !r.contains("falling")),
        "{:?}",
        of_mode(&kept, "balanced").reasons
    );

    // The same board with no keepers: pick 30 really is eighteen past ADP.
    let clean = recommend(&RecommendInputs::new(
        &available,
        Some(&mine),
        &standard,
        3,
        15,
        30,
        12,
    ));
    assert!(
        of_mode(&clean, "balanced")
            .reasons
            .iter()
            .any(|r| r.contains("falling")),
        "{:?}",
        of_mode(&clean, "balanced").reasons
    );
}

// ---------- league size ----------

#[test]
fn the_falling_threshold_is_a_share_of_a_round_not_eight_picks_everywhere() {
    // Eight picks past ADP is two-thirds of a twelve-team round. In a
    // fourteen-team league it is nine, and a player nine picks past his ADP
    // has fallen exactly as far in rounds as one eight picks past in twelve.
    let falling = |teams: u32, market_pick: u32| {
        let mut available = vec![player("wr1", "WR", 40.0)];
        available[0].player.adp = Some(20.0);
        let mine = roster_with(&["RB"], &[("QB", 1), ("WR", 1), ("FLEX", 2)]);
        let standard = RosterRules::new(&slots());
        let inputs = RecommendInputs::new(
            &available,
            Some(&mine),
            &standard,
            3,
            15,
            market_pick,
            teams,
        );
        of_mode(&recommend(&inputs), "balanced")
            .reasons
            .iter()
            .any(|r| r.contains("falling"))
    };
    // Fourteen teams: nine picks past ADP clears the bar, eight does not.
    assert!(falling(14, 30));
    assert!(!falling(14, 29));
    // Ten teams: seven clears it, six does not.
    assert!(falling(10, 28));
    assert!(!falling(10, 27));
    // Twelve is unchanged: nine clears, eight does not.
    assert!(falling(12, 29));
    assert!(!falling(12, 28));
}

// ---------- early depth ----------

#[test]
fn early_depth_is_priced_off_the_slots_the_league_starts() {
    // The old term paid +9 to an empty running back room and +0 to an empty
    // quarterback room, for no reason connected to any league's roster. Now
    // every position is measured against the slots it actually has to fill,
    // so a two-TE league prices an empty tight end room above an empty
    // quarterback room — and nothing gets a head start it did not earn.
    let te_heavy = rules(&["QB", "RB", "WR", "TE", "TE", "FLEX", "BN"]);
    let available = vec![player("te1", "TE", 30.0), player("qb1", "QB", 30.0)];
    // No open starting slots reported, so the need layer is out of the way
    // and the early-depth term is the whole difference between the two.
    let mine = roster_with(&["RB", "WR"], &[]);
    let inputs = RecommendInputs::new(&available, Some(&mine), &te_heavy, 6, 15, 60, 12);
    let ctx = context(&inputs, HashMap::from([("RB", 1), ("WR", 1)]));
    let te = score_candidate(&ctx, &available[0], Mode::Balanced).expect("a TE");
    let qb = score_candidate(&ctx, &available[1], Mode::Balanced).expect("a QB");
    let te_total = te.total;
    // Two TE slots plus a share of the flex against one QB slot, at identical
    // VORP: the tight end leads, by about the slot difference and no more.
    assert!(te.total > qb.total, "TE {} vs QB {}", te.total, qb.total);
    assert!(
        te.total - qb.total < 6.0,
        "TE {} vs QB {}",
        te.total,
        qb.total
    );
    assert!(
        te.into_reasons()
            .iter()
            .any(|r| r.contains("the league starts 2.3")),
        "the term has to name the league's own number"
    );
    // A one-TE league prices the same empty room lower.
    let one_te = rules(&["QB", "RB", "WR", "TE", "FLEX", "BN"]);
    let inputs = RecommendInputs::new(&available, Some(&mine), &one_te, 6, 15, 60, 12);
    let ctx = context(&inputs, HashMap::from([("RB", 1), ("WR", 1)]));
    let thin = score_candidate(&ctx, &available[0], Mode::Balanced).expect("a TE");
    assert!(thin.total < te_total, "{} vs {te_total}", thin.total);
}

// ---------- every reason is on the card ----------

fn context<'a>(inputs: &'a RecommendInputs<'a>, have: HashMap<&'a str, u32>) -> Context<'a> {
    let open: HashMap<String, u32> = inputs
        .my_roster
        .map(|r| r.open_starters.iter().cloned().collect())
        .unwrap_or_default();
    Context {
        inputs,
        open,
        have,
        rb_teams: HashSet::new(),
        need_pressure: 1.0,
        rounds_left: inputs.total_rounds - inputs.current_round + 1,
        median_cv: None,
    }
}

#[test]
fn the_reasons_add_up_to_the_score() {
    // Two terms used to move the total without ever appearing on the card:
    // the -60 that keeps a defence out of round three, and safe mode's
    // discount on bonus-dependent points. A card the user cannot add up is a
    // card the user cannot argue with.
    let mut available = vec![
        player("def1", "DEF", 20.0),
        player("wr1", "WR", 40.0),
        player("k1", "K", 10.0),
    ];
    // A receiver whose value is heavily yardage-bonus driven — the term safe
    // mode docks and used to dock silently.
    available[1].player.bonus_points = 30.0;
    available[1].player.points = 200.0;
    let mine = roster_with(&["RB", "WR"], &[("QB", 1), ("TE", 1), ("FLEX", 2)]);
    let with_kicker = rules(&["QB", "RB", "WR", "TE", "FLEX", "K", "DEF", "BN"]);
    let inputs = RecommendInputs::new(&available, Some(&mine), &with_kicker, 3, 15, 30, 12);
    let have: HashMap<&str, u32> = HashMap::from([("RB", 1), ("WR", 1)]);
    let ctx = context(&inputs, have);

    for mode in [Mode::Balanced, Mode::Safe, Mode::Upside] {
        for candidate in &available {
            let Some(score) = score_candidate(&ctx, candidate, mode) else {
                continue;
            };
            let summed: f64 = score.weights().iter().sum();
            assert!(
                (summed - score.total).abs() < 1e-9,
                "{} in {mode:?}: reasons sum to {summed} but the score is {}",
                candidate.player.player_id,
                score.total
            );
        }
    }
}

#[test]
fn the_early_defence_veto_says_why_it_vetoed() {
    let available: Vec<AvailablePlayer> = vec![player("def1", "DEF", 20.0)];
    let mine = roster_with(&["RB", "WR"], &[("DEF", 1), ("FLEX", 2)]);
    let with_def = rules(&["QB", "RB", "WR", "TE", "FLEX", "DEF", "BN"]);
    let inputs = RecommendInputs::new(&available, Some(&mine), &with_def, 3, 15, 30, 12);
    let ctx = context(&inputs, HashMap::from([("RB", 1), ("WR", 1)]));
    let score = score_candidate(&ctx, &available[0], Mode::Balanced).expect("not disqualified");
    let reasons = score.into_reasons();
    assert!(
        reasons.iter().any(|r| r.contains("too early")),
        "{reasons:?}"
    );
}
