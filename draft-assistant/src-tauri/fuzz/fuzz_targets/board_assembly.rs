#![no_main]
//! Build a full scored board from an arbitrary Sleeper-shaped payload.
//!
//! This is the whole ingestion pipeline — parse, score, rank, tier, compute
//! replacement levels — driven by data the app does not control. It must
//! degrade to warnings, never panic, and never emit a duplicate player.

use draft_assistant_lib::board::build_board;
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use libfuzzer_sys::fuzz_target;
use std::collections::{HashMap, HashSet};

#[derive(serde::Deserialize)]
struct Input {
    league: League,
    draft: Draft,
    season_rows: Vec<ProjectionRow>,
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(input) = serde_json::from_str::<Input>(text) else {
        return;
    };
    // A pathological roster list would only fuzz the allocator.
    if input.league.roster_positions.len() > 64 || input.season_rows.len() > 512 {
        return;
    }

    let rules = RosterRules::new(&input.league.roster_positions);
    let player_meta: HashMap<String, PlayerMeta> = input
        .season_rows
        .iter()
        .filter_map(|row| row.player.clone().map(|p| (row.player_id.clone(), p)))
        .collect();
    let mut warnings = Vec::new();

    let built = build_board(
        &input.league,
        &input.draft,
        &player_meta,
        &input.season_rows,
        &[],
        &rules,
        &mut warnings,
    );

    // Invariants that must hold for any input at all.
    let mut seen = HashSet::new();
    for player in &built.players {
        assert!(
            seen.insert(player.player_id.clone()),
            "duplicate player {} on the board",
            player.player_id
        );
        assert!(player.points.is_finite(), "non-finite points");
        assert!(player.vorp.is_finite(), "non-finite vorp");
        assert!(player.position_rank >= 1, "position_rank must be 1-based");
    }
    for baseline in built.replacement.baseline.values() {
        assert!(baseline.is_finite(), "non-finite replacement baseline");
    }
});
