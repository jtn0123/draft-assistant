//! The imported second opinion, seen from the other end: does it reach the
//! `DraftView` the frontend reads, and does a disagreement that runs the
//! user's way turn into a rec-card reason?
//!
//! `common::fixture()` deliberately carries only small disagreements — a big
//! one changes which player the simulated picks take, which would re-cast
//! every other fixture test — so the big ones are set up here.

mod common;

use draft_assistant_lib::second_opinion::{self, SecondOpinion};
use draft_assistant_lib::view::build_view;

fn fixture_csv() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("second_opinion_sample.csv");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_view_carries_every_players_second_opinion_and_the_load_date() {
    let (loaded, _, config) = common::fixture();
    let view = build_view(&loaded, &config);
    assert_eq!(view.schema_version, "1.3");
    assert_eq!(
        view.data_health.second_opinion_loaded_at,
        Some(1_756_000_000)
    );
    let with_one = view
        .available
        .iter()
        .filter(|a| a.player.second_opinion.is_some())
        .count();
    assert!(with_one > 0, "no player carried a second opinion");
    let w6 = view
        .available
        .iter()
        .find(|a| a.player.player_id == "w6")
        .expect("Third Wideout is on the board");
    let opinion = w6
        .player
        .second_opinion
        .as_ref()
        .expect("w6 has an opinion");
    assert_eq!(opinion.source, "Clay");
    assert_eq!(opinion.positional_rank, 14);
}

#[test]
fn a_board_with_nothing_imported_carries_no_opinion_at_all() {
    let (mut loaded, _, config) = common::fixture();
    for player in &mut loaded.board {
        player.second_opinion = None;
    }
    loaded.second_opinion_loaded_at = None;
    let view = build_view(&loaded, &config);
    assert!(view.data_health.second_opinion_loaded_at.is_none());
    assert!(view
        .available
        .iter()
        .all(|a| a.player.second_opinion.is_none()));
}

#[test]
fn a_big_disagreement_in_the_players_favour_reaches_the_rec_card() {
    let (mut loaded, _, config) = common::fixture();
    // Every player on the board told that Clay rates him twelve positional
    // spots higher than this board does, with an ADP thirty-six picks behind
    // Clay's own overall rank — so whichever one the engine picks, the
    // recommendation it writes has to carry the reason. Thirty-six picks is
    // nine rounds of this four-team fixture league, which is the point: the
    // sentence counts rounds of *this* league, not of a twelve-team one.
    for player in &mut loaded.board {
        player.position_rank = 21;
        player.adp = Some(58.0);
        player.second_opinion = Some(SecondOpinion {
            positional_rank: 9,
            overall_rank: 22,
            source: "Clay".to_string(),
        });
    }
    let view = build_view(&loaded, &config);
    assert!(!view.recommendations.is_empty(), "nothing was recommended");
    for rec in &view.recommendations {
        let reason = rec
            .reasons
            .iter()
            .find(|r| r.starts_with("Clay has him"))
            .unwrap_or_else(|| panic!("no second-opinion reason in {:?}", rec.reasons));
        assert_eq!(
            *reason,
            format!("Clay has him {}9 — market is 9 rounds late", rec.position)
        );
    }
}

#[test]
fn a_disagreement_against_the_player_costs_him_the_card() {
    let (mut loaded, _, config) = common::fixture();
    let before = build_view(&loaded, &config).recommendations[0].clone();
    let player = loaded
        .board
        .iter_mut()
        .find(|p| p.player_id == before.player_id)
        .expect("the recommended player is on the board");
    player.position_rank = 1;
    player.second_opinion = Some(SecondOpinion {
        positional_rank: 30,
        overall_rank: 90,
        source: "Clay".to_string(),
    });
    let view = build_view(&loaded, &config);
    // The imported source has him twenty-nine places behind where this board
    // does. That used to be worth nothing at all — the bump only ever ran one
    // way — and now it is enough to take the best player off the card.
    assert_ne!(
        view.recommendations[0].player_id, before.player_id,
        "a source that is down on him left him top of the card: {:?}",
        view.recommendations[0]
    );
    // Wherever he does still appear, the line reads as the warning it is.
    for rec in view
        .recommendations
        .iter()
        .filter(|r| r.player_id == before.player_id)
    {
        let line = rec
            .reasons
            .iter()
            .find(|r| r.starts_with("Clay has him"))
            .expect("the disagreement is reported");
        assert!(line.contains("behind this board's"), "{line}");
        assert!(!line.contains("late"), "{line}");
    }
}

#[test]
fn importing_the_real_export_stamps_the_board_it_recognises() {
    let (mut loaded, _, config) = common::fixture();
    // The fixture league's players are invented, so nothing in the real export
    // matches them — which is exactly the "0 of 40" case the toast must be
    // able to report without anything breaking.
    let table = second_opinion::parse(&fixture_csv(), 1_700_000_000).expect("the export parses");
    let report = second_opinion::apply(&table, &mut loaded.board);
    assert_eq!(report.total, 38);
    assert_eq!(report.matched, 0);
    // The rows the parser would not rank are reported, not hidden: three
    // whose points came off the script's ADP curve, two defences ranked off
    // a week-one matchup page.
    assert_eq!(report.excluded.total(), 5);
    assert_eq!(
        report.excluded.reason().as_deref(),
        Some("3 estimated from ADP, 2 week-1 defence rankings")
    );

    // Rename one fixture player to a name the export does carry, suffix and
    // all, and he matches on the next pass.
    let rb = loaded
        .board
        .iter_mut()
        .find(|p| p.position == "RB")
        .expect("the fixture has a running back");
    rb.name = "Travis Etienne".to_string();
    rb.team = Some("NO".to_string());
    let report = second_opinion::apply(&table, &mut loaded.board);
    assert_eq!(report.matched, 1);
    let view = build_view(&loaded, &config);
    assert!(view
        .available
        .iter()
        .any(|a| a.player.second_opinion.is_some()));
}
