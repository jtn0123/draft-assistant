//! What the two context blocks actually say.
//!
//! These pin whole lines rather than substrings. The context is a prompt: a
//! line that quietly changes shape is a change to what Claude was told, and
//! that is exactly the kind of edit a test should make somebody look at.

use super::*;
use crate::chat_fixtures::draft_fixture;
use crate::season::{MatchupRow, MatchupView};

/// The line beginning with `prefix`, or a failure naming what was there.
fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting {prefix:?} in:\n{text}"))
}

fn has_line_starting(text: &str, prefix: &str) -> bool {
    text.lines().any(|l| l.starts_with(prefix))
}

#[test]
fn a_plain_league_says_nothing_about_keepers_trades_or_a_reversal() {
    let context = draft_context(&draft_fixture());
    assert_eq!(
        line(&context, "League:"),
        "League: The League (10 teams, 15 rounds, season 2026)"
    );
    assert_eq!(
        line(&context, "Now:"),
        "Now: round 3, pick 24, on the clock Dana. Your slot: 3. Your next picks: [28, 43, 48, 63]"
    );
    // No rule lines, and no empty "Round prices" heading either.
    assert!(!has_line_starting(&context, "Keepers:"), "{context}");
    assert!(!has_line_starting(&context, "Traded picks:"), "{context}");
    assert!(!has_line_starting(&context, "Third-round"), "{context}");
    assert!(!has_line_starting(&context, "Round prices"), "{context}");
    // The board still leads with the head of the list, now carrying the bye
    // week and the injury tag a start/sit answer needs.
    assert_eq!(
        line(&context, "11."),
        "11. Ladd McConkey WR — 214 pts, VORP 114, T4, ADP 11, survives 39%, bye 9"
    );
}

#[test]
fn the_league_says_how_it_scores_and_what_it_starts() {
    // Without these, every board is read through full-PPR habits and a roster
    // shape is guessed from the position mix.
    let context = draft_context(&draft_fixture());
    assert_eq!(
        line(&context, "Scoring:"),
        "Scoring: half PPR (0.50 per catch), 4 per passing TD"
    );
    assert_eq!(line(&context, "Roster:"), "Roster: QBx1 RBx2 FLEXx1 BNx2");
}

#[test]
fn a_te_premium_league_says_so() {
    let mut view = draft_fixture();
    view.league.scoring_settings.insert("rec".into(), 1.0);
    view.league
        .scoring_settings
        .insert("bonus_rec_te".into(), 0.5);
    assert_eq!(
        line(&draft_context(&view), "Scoring:"),
        "Scoring: full PPR (1.00 per catch), TE premium +0.50 per catch, 4 per passing TD"
    );
}

#[test]
fn an_injury_tag_travels_with_the_player() {
    let mut view = draft_fixture();
    view.available[0].player.injury_status = Some("Q".into());
    view.available[0].player.bye_week = None;
    assert_eq!(
        line(&draft_context(&view), "11."),
        "11. Ladd McConkey WR — 214 pts, VORP 114, T4, ADP 11, survives 39%, bye - (Q)"
    );
}

#[test]
fn kickers_and_defences_stay_off_the_board_until_the_last_rounds() {
    let mut view = draft_fixture();
    let mut defence = view.available[0].clone();
    defence.player.player_id = "LAR".into();
    defence.player.name = "Los Angeles Rams".into();
    defence.player.position = "DEF".into();
    defence.player.overall_rank = 64;
    view.available.insert(0, defence);
    // Round 3 of fifteen: not a word about defences.
    assert!(
        !draft_context(&view).contains("Los Angeles Rams"),
        "{}",
        draft_context(&view)
    );
    // Round 14 of fifteen: now it is the pick that is actually due.
    view.draft.current_round = 14;
    assert!(draft_context(&view).contains("Los Angeles Rams"));
}

#[test]
fn a_round_that_priced_at_nothing_is_left_out_rather_than_reported_as_free() {
    // The price is clamped at zero, so a round whose median pick was a
    // below-replacement body prices at zero — which is the floor talking, not
    // the round being worthless.
    let mut view = draft_fixture();
    view.pick_prices = vec![
        crate::pick_value::PickPrice {
            round: 1,
            points: 92.4,
            example: None,
        },
        crate::pick_value::PickPrice {
            round: 2,
            points: 0.0,
            example: None,
        },
    ];
    assert_eq!(
        line(&draft_context(&view), "Round prices"),
        "Round prices so far (points over replacement the round actually took): R1 92"
    );
    // And a table of nothing but zeroes raises no heading at all.
    for price in &mut view.pick_prices {
        price.points = 0.0;
    }
    assert!(!has_line_starting(&draft_context(&view), "Round prices"));
}

#[test]
fn keepers_trades_and_a_reversal_each_get_their_line() {
    let mut view = draft_fixture();
    let teams = view.draft.teams;
    // A keeper of mine at 3 and somebody else's at 1.
    view.draft.keeper_picks = vec![1, 3];
    // Third-round reversal: rounds 3 up read backwards.
    for round in 3..=view.draft.rounds {
        for index in 0..teams {
            let pick = (round - 1) * teams + index + 1;
            let plain = (pick - 1) % teams + 1;
            let plain = if (round - 1) % 2 == 0 {
                plain
            } else {
                teams + 1 - plain
            };
            view.draft
                .pick_slot_overrides
                .insert(pick, teams + 1 - plain);
        }
    }
    // Then a trade on top: my second-rounder for their third.
    view.draft.pick_slot_overrides.insert(18, 1);
    view.draft.pick_slot_overrides.insert(21, 3);

    let context = draft_context(&view);
    assert_eq!(
        line(&context, "Keepers:"),
        "Keepers: 2 picks league-wide are already spent — yours at 3."
    );
    assert_eq!(
        line(&context, "Traded picks:"),
        "Traded picks: you gained 21; you lost 18."
    );
    assert_eq!(
        line(&context, "Third-round"),
        "Third-round reversal: the order flips at round 3 instead of snaking, so it repeats the round before."
    );
}

#[test]
fn a_keeper_that_is_not_mine_is_counted_without_being_claimed() {
    let mut view = draft_fixture();
    view.draft.keeper_picks = vec![1];
    assert_eq!(
        line(&draft_context(&view), "Keepers:"),
        "Keepers: 1 picks league-wide are already spent — none of them yours."
    );
}

#[test]
fn a_long_trade_list_is_clipped_rather_than_crowding_out_the_board() {
    let mut view = draft_fixture();
    // Ten picks acquired: the list shows eight and counts the rest.
    for pick in [1, 2, 4, 5, 6, 7, 8, 9, 10, 11] {
        view.draft.pick_slot_overrides.insert(pick, 3);
    }
    assert_eq!(
        line(&draft_context(&view), "Traded picks:"),
        "Traded picks: you gained 1, 2, 4, 5, 6, 7, 8, 9 and 2 more."
    );
}

#[test]
fn round_prices_appear_once_the_draft_has_taught_the_board_something() {
    let mut view = draft_fixture();
    view.pick_prices = vec![
        crate::pick_value::PickPrice {
            round: 1,
            points: 92.4,
            example: Some("Bijan Robinson".into()),
        },
        crate::pick_value::PickPrice {
            round: 2,
            points: 71.0,
            example: None,
        },
    ];
    assert_eq!(
        line(&draft_context(&view), "Round prices"),
        "Round prices so far (points over replacement the round actually took): R1 92, R2 71"
    );
}

// ---------- season ----------

fn row(
    slot: &str,
    mine: (&str, Option<&str>, f64),
    theirs: (&str, Option<&str>, f64),
) -> MatchupRow {
    MatchupRow {
        slot: slot.into(),
        my_player_id: Some(format!("mine-{}", mine.0)),
        my_name: mine.0.into(),
        my_team: Some("SF".into()),
        my_injury: mine.1.map(str::to_string),
        my_points: mine.2,
        opp_player_id: Some(format!("theirs-{}", theirs.0)),
        opp_name: theirs.0.into(),
        opp_team: Some("KC".into()),
        opp_injury: theirs.1.map(str::to_string),
        opp_points: theirs.2,
        margin: mine.2 - theirs.2,
    }
}

fn matchup() -> MatchupView {
    let best = vec![
        row("QB", ("Daniels", None, 21.4), ("Mahomes", Some("Q"), 23.0)),
        row("RB", ("Bijan", Some("Q"), 17.2), ("Gibbs", None, 18.8)),
    ];
    let mut set = best.clone();
    // The lineup actually set benches Bijan for a healthy but worse player.
    set[1] = row("RB", ("Hubbard", None, 11.0), ("Gibbs", None, 18.8));
    MatchupView {
        my_name: "Me".into(),
        opp_name: "Them".into(),
        my_avatar: None,
        opp_avatar: None,
        my_projected: 38.6,
        opp_projected: 41.8,
        rows: best,
        set_rows: set,
        set_projected: 32.4,
    }
}

#[test]
fn the_lineup_block_tags_injuries_on_both_sides() {
    let block = lineup_block(&matchup(), 6.2);
    assert_eq!(line(&block, "QB:"), "QB: Daniels 21.4 vs Mahomes (Q) 23.0");
    assert_eq!(line(&block, "RB:"), "RB: Bijan (Q) 17.2 vs Gibbs 18.8");
}

#[test]
fn the_lineup_block_separates_the_set_lineup_from_the_best_one() {
    let block = lineup_block(&matchup(), 6.2);
    assert!(
        block.starts_with("Best lineup (slot, yours, proj, theirs, proj; Q/D/O = injury tag):\n"),
        "{block}"
    );
    assert_eq!(
        line(&block, "Your lineup as set"),
        "Your lineup as set projects 32.4 against a best of 38.6 — 6.2 left on the table."
    );
    assert_eq!(
        line(&block, "Started but not"),
        "Started but not in the best lineup: RB Hubbard"
    );
}

#[test]
fn an_empty_starting_slot_is_not_reported_as_a_benched_player() {
    // A manager who left a slot empty has a row with no player in it. It used
    // to come out of here as "K " — a benched player with a blank name.
    let mut view = matchup();
    let mut empty = view.set_rows[1].clone();
    empty.my_player_id = None;
    empty.my_name = String::new();
    empty.slot = "K".into();
    view.set_rows.push(empty);
    let block = lineup_block(&view, 6.2);
    assert_eq!(
        line(&block, "Started but not"),
        "Started but not in the best lineup: RB Hubbard"
    );
}

#[test]
fn a_lineup_that_is_already_the_best_one_names_nothing_to_change() {
    let mut view = matchup();
    view.set_rows = view.rows.clone();
    view.set_projected = view.my_projected;
    let block = lineup_block(&view, 0.0);
    assert!(!has_line_starting(&block, "Started but not"), "{block}");
    assert_eq!(
        line(&block, "Your lineup as set"),
        "Your lineup as set projects 38.6 against a best of 38.6 — 0.0 left on the table."
    );
}

#[test]
fn the_screens_get_their_own_suggestions() {
    assert!(suggestions("season")[0].contains("start"));
    assert!(suggestions("draft")[0].contains("TE"));
}
