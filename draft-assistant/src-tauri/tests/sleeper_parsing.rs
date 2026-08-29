//! Parsing tests for the Sleeper wire types.
//!
//! Sleeper's projection endpoint is undocumented and its league/draft payloads
//! carry fields we do not model. These pin the tolerances the app relies on:
//! unknown fields ignored, absent optional fields defaulted, and a malformed
//! payload rejected as an error rather than a panic.

use draft_assistant_lib::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow};

#[test]
fn unknown_upstream_fields_are_ignored() {
    // Sleeper adds fields regularly; a new one must not break the parse.
    let json = r#"{
        "league_id": "1", "name": "L", "season": "2026", "status": "drafting",
        "total_rosters": 12,
        "roster_positions": ["QB", "RB", "BN"], "scoring_settings": {"rec": 1.0},
        "draft_id": "9",
        "a_field_sleeper_added_last_tuesday": {"nested": [1, 2, 3]}
    }"#;
    let league: League = serde_json::from_str(json).expect("unknown fields must not break parsing");
    assert_eq!(league.total_rosters, 12);
    assert_eq!(league.roster_positions.len(), 3);
}

#[test]
fn optional_draft_settings_default_when_absent() {
    // A league draft omits the slots_* keys that mock drafts carry.
    let json = r#"{
        "draft_id": "9", "status": "drafting", "type": "snake",
        "settings": {"teams": 12, "rounds": 15}
    }"#;
    let draft: Draft = serde_json::from_str(json).expect("minimal draft must parse");
    assert_eq!(draft.settings.teams, 12);
    assert_eq!(draft.settings.pick_timer, None);
    assert_eq!(draft.settings.slots_qb, None);
    assert!(draft.draft_order.is_none());
}

#[test]
fn a_missing_required_field_is_an_error_not_a_panic() {
    // `teams` has no default: its absence must surface as a parse error that
    // the caller turns into a warning, not a silent zero.
    let json = r#"{"draft_id": "9", "status": "drafting", "type": "snake",
                   "settings": {"rounds": 15}}"#;
    assert!(serde_json::from_str::<Draft>(json).is_err());
}

#[test]
fn garbage_input_is_rejected_cleanly() {
    for bad in [
        "",
        "null",
        "[]",
        "{",
        "\u{feff}{}",
        r#"{"league_id": 12345}"#, // wrong type
    ] {
        assert!(
            serde_json::from_str::<League>(bad).is_err(),
            "expected {bad:?} to be rejected"
        );
    }
}

#[test]
fn projection_rows_tolerate_absent_stats_and_players() {
    let json = r#"[
        {"player_id": "1", "stats": {"pts_ppr": 210.5}, "player": {"first_name": "A",
         "last_name": "B", "position": "WR", "team": "NO"}},
        {"player_id": "2", "stats": {}},
        {"player_id": "3"}
    ]"#;
    let rows: Vec<ProjectionRow> =
        serde_json::from_str(json).expect("partial projection rows must parse");
    assert_eq!(rows.len(), 3);
    assert!(rows[0].player.is_some());
    assert!(rows[2].player.is_none());
}

#[test]
fn a_pick_without_metadata_still_parses() {
    // Manual picks we write ourselves carry no metadata block.
    let json = r#"{"round": 1, "pick_no": 3, "draft_slot": 3, "player_id": "8144"}"#;
    let pick: Pick = serde_json::from_str(json).expect("bare pick must parse");
    assert_eq!(pick.pick_no, 3);
    assert!(pick.metadata.is_none());
    assert!(pick.picked_by.is_none());
}

#[test]
fn player_metadata_tolerates_missing_position_and_team() {
    // Free agents and retired players come back with null team/position.
    let json = r#"{"first_name": "A", "last_name": "B", "team": null, "position": null}"#;
    let meta: PlayerMeta = serde_json::from_str(json).expect("null fields must parse");
    assert!(meta.position.is_none());
    assert!(meta.team.is_none());
}
