//! The draft context's situational blocks.
//!
//! `chat_context_tests.rs` pins the league rules and the board. These are the
//! three blocks that only appear when the draft is doing something — a tier
//! emptying, a run on a position, picks landing — and which Claude therefore
//! only ever reads in the situations where they change the answer.

use super::*;
use crate::chat_fixtures::draft_fixture;
use crate::view::{PositionRun, RecentPick, TierAlert};

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting {prefix:?} in:\n{text}"))
}

#[test]
fn a_tier_about_to_empty_is_called_out_by_position() {
    let mut view = draft_fixture();
    view.tier_alerts = vec![
        TierAlert {
            position: "RB".into(),
            tier: 2,
            players_left: 1,
        },
        TierAlert {
            position: "TE".into(),
            tier: 1,
            players_left: 2,
        },
    ];
    let context = draft_context(&view);
    assert_eq!(
        line(&context, "Tier alerts:"),
        "Tier alerts: RB T2 has 1 left; TE T1 has 2 left"
    );
}

#[test]
fn a_run_on_a_position_is_reported_with_the_window_it_was_measured_over() {
    let mut view = draft_fixture();
    view.position_run = Some(PositionRun {
        position: "WR".into(),
        count: 5,
        window: 8,
    });
    let context = draft_context(&view);
    assert_eq!(
        line(&context, "Position run"),
        "Position run in progress: WR (5 of the last 8 picks)"
    );
}

#[test]
fn the_last_eight_picks_are_listed_so_a_run_can_be_seen() {
    let mut view = draft_fixture();
    view.recent_picks = (1..=10)
        .map(|n| RecentPick {
            pick_no: n,
            round: 1,
            slot: n,
            slot_name: None,
            player_id: format!("p{n}"),
            name: format!("Player {n}"),
            position: "WR".into(),
            team: None,
        })
        .collect();
    let context = draft_context(&view);
    let recent = line(&context, "Recent picks:");
    // Eight, in the order they were taken — enough to see a run without
    // spending the prompt on the whole draft so far.
    assert!(
        recent.starts_with("Recent picks: 1 Player 1 (WR)"),
        "{recent}"
    );
    assert!(recent.ends_with("8 Player 8 (WR)"), "{recent}");
}
