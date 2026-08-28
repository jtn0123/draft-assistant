use draft_assistant_lib::board::build_board;
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct BoardFixture {
    league: League,
    draft: Draft,
    season_rows: Vec<ProjectionRow>,
}

#[test]
fn sleeper_shaped_fixture_builds_mixed_board_with_kicker() {
    let fixture: BoardFixture = serde_json::from_str(include_str!("fixtures/board_input.json"))
        .expect("sanitized Sleeper fixture must deserialize");
    let rules = RosterRules::new(&fixture.league.roster_positions);
    let player_meta: HashMap<String, PlayerMeta> = fixture
        .season_rows
        .iter()
        .filter_map(|row| {
            row.player
                .clone()
                .map(|player| (row.player_id.clone(), player))
        })
        .collect();
    let mut warnings = Vec::new();

    let result = build_board(
        &fixture.league,
        &fixture.draft,
        &player_meta,
        &fixture.season_rows,
        &[],
        &rules,
        &mut warnings,
    );

    let positions: HashSet<&str> = result
        .players
        .iter()
        .map(|player| player.position.as_str())
        .collect();
    assert_eq!(
        positions,
        HashSet::from(["QB", "RB", "WR", "TE", "K", "DEF"])
    );
    assert!(result.players.iter().all(|player| player.points >= 20.0));
    assert!(result.replacement.demand.contains_key("K"));
    assert!(result.replacement.baseline.contains_key("QB"));
    assert!(warnings.is_empty(), "{warnings:?}");
}
