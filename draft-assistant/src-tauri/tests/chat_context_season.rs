//! What the season screen tells Claude.
//!
//! `season_context` is the whole of what a question on the in-season screen
//! is answered from: if a block is missing, Claude answers from nothing. The
//! fixture is a real four-team season, run through the same `build_season_view`
//! the screen uses, so what is asserted here is the text a live question
//! actually carries.

mod common;

use draft_assistant_lib::chat_context::season_context;
use draft_assistant_lib::season::build_season_view;

fn context() -> String {
    let (loaded, season, config) = common::fixture();
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    season_context(&view)
}

/// The same context with an empty scoreboard, i.e. before anything has kicked
/// off. The fixture's live game started in the second quarter, and a start/sit
/// call about two players already on the field is no longer a call — so a test
/// about the advice has to be taken at a moment the advice can be acted on.
fn context_before_kickoff() -> String {
    let (loaded, mut season, config) = common::fixture();
    std::sync::Arc::make_mut(&mut season.scores).clear();
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    season_context(&view)
}

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting {prefix:?} in:\n{text}"))
}

#[test]
fn the_context_opens_with_the_league_and_the_week_it_is_about() {
    let context = context();
    assert_eq!(
        line(&context, "League:"),
        "League: Fixture League — week 2 of season 2025"
    );
}

#[test]
fn this_weeks_game_carries_both_projections_and_both_sets_of_odds() {
    let context = context();
    let this_week = line(&context, "This week:");
    assert!(this_week.contains(" vs "), "{this_week}");
    assert!(this_week.contains("projected"), "{this_week}");
    // Both odds are percentages, and both have to be there: "will I win" and
    // "does it matter" are different questions and Claude gets asked each.
    assert!(this_week.contains("Win odds "), "{this_week}");
    assert!(this_week.contains("playoff odds "), "{this_week}");
}

#[test]
fn the_lineup_block_comes_with_what_the_set_lineup_is_giving_up() {
    let context = context();
    assert!(
        context.contains("Best lineup (slot, yours, proj, theirs, proj"),
        "{context}"
    );
    let table = line(&context, "Your lineup as set projects");
    assert!(table.contains("left on the table"), "{table}");
}

#[test]
fn every_start_sit_call_is_offered_with_the_reason_for_it() {
    let context = context_before_kickoff();
    assert!(
        context.contains("Start/sit calls available:"),
        "the fixture has a better lineup than the one set:\n{context}"
    );
    // A call without its "why" is an instruction rather than an argument, and
    // Claude would have nothing to weigh against what the manager already
    // decided.
    let call = context
        .lines()
        .find(|l| l.contains(" over ") && l.contains(" for "))
        .unwrap_or_else(|| panic!("no start/sit line in:\n{context}"));
    assert!(call.contains(" — "), "{call}");
}

#[test]
fn the_standings_are_listed_by_seed_with_each_teams_playoff_odds() {
    let context = context();
    assert!(context.contains("Standings (seed, team, record, playoff odds):"));
    let first = line(&context, "1. ");
    assert!(first.ends_with('%'), "{first}");
    // Four teams in, four teams out.
    let seeds = (1..=4)
        .filter(|seed| context.lines().any(|l| l.starts_with(&format!("{seed}. "))))
        .count();
    assert_eq!(seeds, 4, "{context}");
}
