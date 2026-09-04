//! What each poll tick decides, tested without a running app. Its own file
//! only because `poll.rs` is at the line cap.

use super::*;
use crate::season_live::{GameChip, GameState, PlayState};

/// `n` picks, each of player `id{i}` unless `swap` renames one of them.
fn picks(n: u32, swap: Option<(u32, &str)>) -> Vec<Pick> {
    (1..=n)
        .map(|pick_no| Pick {
            round: 1,
            pick_no,
            draft_slot: pick_no,
            player_id: match swap {
                Some((at, id)) if at == pick_no => id.to_string(),
                _ => format!("id{pick_no}"),
            },
            picked_by: None,
            metadata: None,
            is_keeper: None,
        })
        .collect()
}

#[test]
fn the_first_tick_always_counts_as_a_change() {
    let mut memory = DraftPollMemory::default();
    assert!(
        memory.picks_changed(&picks(0, None)),
        "the initial state must reach the UI"
    );
    assert!(memory.status_changed("pre_draft"));
}

#[test]
fn an_identical_response_is_not_a_change() {
    let mut memory = DraftPollMemory::default();
    memory.picks_changed(&picks(26, None));
    assert!(!memory.picks_changed(&picks(26, None)));
    assert!(
        memory.picks_changed(&picks(27, None)),
        "a new pick is a change"
    );
    assert!(!memory.picks_changed(&picks(27, None)));
}

#[test]
fn a_commissioner_swapping_a_pick_is_a_change_at_the_same_count() {
    // The bug this guards: pick 14 is edited to a different player, the
    // count never moves, and the board silently keeps the old name.
    let mut memory = DraftPollMemory::default();
    assert!(memory.picks_changed(&picks(26, None)));
    assert!(
        memory.picks_changed(&picks(26, Some((14, "someone-else")))),
        "an edited pick must reach the UI even at an unchanged count"
    );
    assert!(!memory.picks_changed(&picks(26, Some((14, "someone-else")))));
    assert!(
        memory.picks_changed(&picks(26, None)),
        "and undoing the edit is a change too"
    );
}

#[test]
fn a_status_move_is_a_change_even_with_no_new_pick() {
    let mut memory = DraftPollMemory::default();
    memory.status_changed("drafting");
    assert!(!memory.status_changed("drafting"));
    assert!(memory.status_changed("complete"));
}

#[test]
fn scores_that_have_not_moved_do_not_emit() {
    let mut gate = LiveEmitGate::default();
    let quiet = &[][..];
    assert!(
        gate.should_emit(101.4, 98.2, quiet, 1),
        "the first view must be sent"
    );
    assert!(!gate.should_emit(101.4, 98.2, quiet, 1));
    // Below a hundredth of a point is not a score change.
    assert!(!gate.should_emit(101.4001, 98.2001, quiet, 1));
    assert!(gate.should_emit(101.5, 98.2, quiet, 1));
    assert!(
        gate.should_emit(101.5, 98.3, quiet, 1),
        "the opponent moving counts too"
    );
}

/// A game kicking off, the clock running, an NFL score changing, a starter
/// swapped — none of them move the fantasy totals at 0 - 0, and the screen
/// used to sit frozen through all of it.
#[test]
fn the_scoreboard_moving_at_nil_nil_still_emits() {
    let mut gate = LiveEmitGate::default();
    let mut game = LiveGame {
        game_id: "aaa-bbb".to_string(),
        away: "AAA".to_string(),
        home: "BBB".to_string(),
        away_score: None,
        home_score: None,
        state: GameState::Pre,
        status: String::new(),
        kickoff_ms: 0,
        flag: None,
        channel: None,
        chips: vec![GameChip {
            player_id: "rb-1".to_string(),
            name: "A Runner".to_string(),
            slot: "RB".to_string(),
            team: Some("AAA".to_string()),
            points: 0.0,
            is_mine: true,
            state: PlayState::Pre,
        }],
    };
    assert!(gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1));
    assert!(!gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1));

    game.state = GameState::Live;
    game.status = "Q1 14:52".to_string();
    game.chips[0].state = PlayState::Playing;
    assert!(
        gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1),
        "kickoff must reach the screen even before anybody scores"
    );

    game.away_score = Some(3);
    assert!(
        gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1),
        "a field goal by a defence nobody rosters still moves the game"
    );

    game.chips[0].slot = "BN".to_string();
    assert!(
        gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1),
        "a starter moving to the bench changes what the screen shows"
    );
    assert!(!gate.should_emit(0.0, 0.0, std::slice::from_ref(&game), 1));
}

/// The bug behind this: on a quiet Tuesday every score is 0 - 0 and every
/// scoreboard row is identical all day, so the gate saw nothing move — and the
/// analysis rebuilt every twenty ticks (new waiver targets, new trade ideas,
/// new playoff odds) was computed and then silently dropped.
#[test]
fn a_rebuilt_analysis_reaches_the_screen_even_with_nothing_scored() {
    let mut gate = LiveEmitGate::default();
    let quiet = &[][..];
    assert!(
        gate.should_emit(0.0, 0.0, quiet, 1),
        "the first view is sent"
    );
    assert!(
        !gate.should_emit(0.0, 0.0, quiet, 1),
        "the same analysis at the same score is not news"
    );
    assert!(
        gate.should_emit(0.0, 0.0, quiet, 2),
        "a fresh analysis is news even at 0 - 0"
    );
    assert!(!gate.should_emit(0.0, 0.0, quiet, 2));
}
