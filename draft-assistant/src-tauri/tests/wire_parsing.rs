//! Deserialization tests for the Sleeper wire types.
//!
//! Every fixture is a hand-written JSON string shaped like a real Sleeper
//! response, including unknown fields (which must be ignored) and missing
//! optional fields (which must default).

use draft_assistant_lib::sleeper::{
    Draft, League, LeagueUser, Pick, PlayerMeta, ProjectionRow, SleeperClient,
};
use std::collections::HashMap;

#[test]
fn league_parses_full_payload() {
    let json = r#"{
        "league_id": "123456789",
        "name": "Test League",
        "season": "2025",
        "status": "in_season",
        "total_rosters": 12,
        "roster_positions": ["QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "K", "DEF", "BN"],
        "scoring_settings": {"pass_td": 4.0, "rec": 1.0, "rush_yd": 0.1},
        "draft_id": "draft-1",
        "previous_league_id": "987654321",
        "settings": {
            "playoff_week_start": 15,
            "playoff_teams": 6,
            "waiver_budget": 100,
            "start_week": 1,
            "some_unknown_knob": 7
        },
        "unknown_top_level": {"ignored": true}
    }"#;
    let league: League = serde_json::from_str(json).unwrap();
    assert_eq!(league.league_id, "123456789");
    assert_eq!(league.name, "Test League");
    assert_eq!(league.season, "2025");
    assert_eq!(league.status, "in_season");
    assert_eq!(league.total_rosters, 12);
    assert_eq!(league.roster_positions.len(), 10);
    assert_eq!(league.scoring_settings.get("rec"), Some(&1.0));
    assert_eq!(league.draft_id.as_deref(), Some("draft-1"));
    assert_eq!(league.previous_league_id.as_deref(), Some("987654321"));
    assert_eq!(league.settings.playoff_week_start, Some(15));
    assert_eq!(league.settings.playoff_teams, Some(6));
    assert_eq!(league.settings.waiver_budget, Some(100.0));
    assert_eq!(league.settings.start_week, Some(1));
}

#[test]
fn league_minimal_payload_defaults_everything_optional() {
    let json = r#"{
        "league_id": "1",
        "name": "Bare",
        "season": "2025",
        "status": "drafting",
        "total_rosters": 10,
        "roster_positions": ["QB"],
        "scoring_settings": {}
    }"#;
    let league: League = serde_json::from_str(json).unwrap();
    assert!(league.draft_id.is_none());
    assert!(league.previous_league_id.is_none());
    assert!(league.settings.playoff_week_start.is_none());
    assert!(league.settings.playoff_teams.is_none());
    assert!(league.settings.waiver_budget.is_none());
    assert!(league.settings.start_week.is_none());
}

fn league_with_playoff_start(start: Option<u32>) -> League {
    let mut json = serde_json::json!({
        "league_id": "1",
        "name": "L",
        "season": "2025",
        "status": "in_season",
        "total_rosters": 10,
        "roster_positions": ["QB"],
        "scoring_settings": {}
    });
    if let Some(week) = start {
        json["settings"] = serde_json::json!({ "playoff_week_start": week });
    }
    serde_json::from_value(json).unwrap()
}

#[test]
fn last_regular_week_defaults_to_14() {
    assert_eq!(league_with_playoff_start(None).last_regular_week(), 14);
}

#[test]
fn last_regular_week_is_the_week_before_playoffs() {
    assert_eq!(league_with_playoff_start(Some(15)).last_regular_week(), 14);
    assert_eq!(league_with_playoff_start(Some(10)).last_regular_week(), 9);
    assert_eq!(league_with_playoff_start(Some(18)).last_regular_week(), 17);
}

#[test]
fn last_regular_week_ignores_degenerate_playoff_starts() {
    // 0 and 1 would make the regular season empty; fall back to the default.
    assert_eq!(league_with_playoff_start(Some(0)).last_regular_week(), 14);
    assert_eq!(league_with_playoff_start(Some(1)).last_regular_week(), 14);
}

#[test]
fn draft_parses_mock_draft_payload() {
    let json = r#"{
        "draft_id": "mock-42",
        "status": "drafting",
        "type": "snake",
        "settings": {
            "teams": 12,
            "rounds": 15,
            "pick_timer": 60,
            "slots_qb": 1,
            "slots_rb": 2,
            "slots_wr": 2,
            "slots_te": 1,
            "slots_flex": 1,
            "slots_super_flex": 0,
            "slots_k": 1,
            "slots_def": 1,
            "reversal_round": 0
        },
        "draft_order": {"user-a": 1, "user-b": 2},
        "start_time": 1756500000000,
        "season": "2025",
        "metadata": {"name": "Mock of Champions", "scoring_type": "half_ppr"},
        "creators": ["guest-1"],
        "last_picked": 1756500123456
    }"#;
    let draft: Draft = serde_json::from_str(json).unwrap();
    assert_eq!(draft.draft_id, "mock-42");
    assert_eq!(draft.status, "drafting");
    assert_eq!(draft.draft_type, "snake");
    assert_eq!(draft.settings.teams, 12);
    assert_eq!(draft.settings.rounds, 15);
    assert_eq!(draft.settings.pick_timer, Some(60));
    assert_eq!(draft.settings.slots_qb, Some(1));
    assert_eq!(draft.settings.slots_super_flex, Some(0));
    assert_eq!(draft.settings.slots_def, Some(1));
    assert_eq!(draft.draft_order.as_ref().unwrap().get("user-b"), Some(&2));
    assert_eq!(draft.start_time, Some(1_756_500_000_000));
    assert_eq!(draft.season.as_deref(), Some("2025"));
    let meta = draft.metadata.as_ref().unwrap();
    assert_eq!(meta.name.as_deref(), Some("Mock of Champions"));
    assert_eq!(meta.scoring_type.as_deref(), Some("half_ppr"));
    assert_eq!(draft.creators.as_ref().unwrap()[0], "guest-1");
    assert_eq!(draft.last_picked, Some(1_756_500_123_456));
}

#[test]
fn draft_minimal_payload_defaults_everything_optional() {
    let json = r#"{
        "draft_id": "d1",
        "status": "complete",
        "type": "linear",
        "settings": {"teams": 10, "rounds": 16}
    }"#;
    let draft: Draft = serde_json::from_str(json).unwrap();
    assert!(draft.settings.pick_timer.is_none());
    assert!(draft.settings.slots_qb.is_none());
    assert!(draft.settings.slots_flex.is_none());
    assert!(draft.draft_order.is_none());
    assert!(draft.start_time.is_none());
    assert!(draft.season.is_none());
    assert!(draft.metadata.is_none());
    assert!(draft.creators.is_none());
    assert!(draft.last_picked.is_none());
}

#[test]
fn picks_parse_with_and_without_metadata() {
    let json = r#"[
        {
            "round": 1,
            "pick_no": 1,
            "draft_slot": 1,
            "player_id": "4034",
            "picked_by": "user-a",
            "metadata": {
                "first_name": "Christian",
                "last_name": "McCaffrey",
                "position": "RB",
                "team": "SF",
                "years_exp": "8"
            }
        },
        {"round": 1, "pick_no": 2, "draft_slot": 2, "player_id": "6786"}
    ]"#;
    let picks: Vec<Pick> = serde_json::from_str(json).unwrap();
    assert_eq!(picks.len(), 2);
    assert_eq!(picks[0].pick_no, 1);
    assert_eq!(picks[0].draft_slot, 1);
    assert_eq!(picks[0].player_id, "4034");
    assert_eq!(picks[0].picked_by.as_deref(), Some("user-a"));
    let meta = picks[0].metadata.as_ref().unwrap();
    assert_eq!(meta.first_name.as_deref(), Some("Christian"));
    assert_eq!(meta.last_name.as_deref(), Some("McCaffrey"));
    assert_eq!(meta.position.as_deref(), Some("RB"));
    assert_eq!(meta.team.as_deref(), Some("SF"));
    assert!(picks[1].picked_by.is_none());
    assert!(picks[1].metadata.is_none());
}

#[test]
fn players_dictionary_parses_and_defaults_missing_fields() {
    // Shaped like /v1/players/nfl: keyed by player id, with team defenses
    // keyed by team code and carrying almost none of the usual fields.
    let json = r##"{
        "4034": {
            "full_name": "Christian McCaffrey",
            "first_name": "Christian",
            "last_name": "McCaffrey",
            "position": "RB",
            "team": "SF",
            "fantasy_positions": ["RB"],
            "injury_status": "Questionable",
            "years_exp": 8,
            "age": 29,
            "espn_id": 3117251,
            "hashtag": "#christianmccaffrey-NFL-SF-23"
        },
        "SEA": {"position": "DEF", "team": "SEA"}
    }"##;
    let players: HashMap<String, PlayerMeta> = serde_json::from_str(json).unwrap();
    let cmc = &players["4034"];
    assert_eq!(cmc.full_name.as_deref(), Some("Christian McCaffrey"));
    assert_eq!(cmc.position.as_deref(), Some("RB"));
    assert_eq!(cmc.fantasy_positions.as_deref(), Some(&["RB".into()][..]));
    assert_eq!(cmc.injury_status.as_deref(), Some("Questionable"));
    assert_eq!(cmc.years_exp, Some(8));
    assert_eq!(cmc.age, Some(29));
    let sea = &players["SEA"];
    assert!(sea.full_name.is_none());
    assert!(sea.first_name.is_none());
    assert!(sea.fantasy_positions.is_none());
    assert!(sea.injury_status.is_none());
    assert!(sea.years_exp.is_none());
    assert_eq!(sea.position.as_deref(), Some("DEF"));
}

#[test]
fn projection_rows_parse_stats_adp_and_bye_weeks() {
    let json = r#"[
        {
            "player_id": "4034",
            "stats": {"rush_yd": 1100.5, "rec": 55.0, "adp_ppr": 3.2, "adp_half_ppr": 3.8},
            "player": {"full_name": "Christian McCaffrey", "position": "RB", "team": "SF"},
            "week": 4,
            "opponent": "LAR",
            "company": "sportradar"
        },
        {"player_id": "4034", "week": 9, "opponent": null},
        {"player_id": "9999"}
    ]"#;
    let rows: Vec<ProjectionRow> = serde_json::from_str(json).unwrap();
    assert_eq!(rows.len(), 3);
    let full = &rows[0];
    assert_eq!(full.player_id, "4034");
    assert_eq!(full.stat("rush_yd"), Some(1100.5));
    assert_eq!(full.stat("adp_ppr"), Some(3.2));
    assert_eq!(full.stat("pass_td"), None, "absent stat key");
    assert_eq!(full.week, Some(4));
    assert_eq!(full.opponent.as_deref(), Some("LAR"));
    assert_eq!(
        full.player.as_ref().unwrap().full_name.as_deref(),
        Some("Christian McCaffrey")
    );
    let bye = &rows[1];
    assert!(bye.opponent.is_none(), "null opponent means bye");
    assert_eq!(bye.stat("rec"), None, "no stats map at all");
    assert!(rows[2].stats.is_none());
    assert!(rows[2].player.is_none());
    assert!(rows[2].week.is_none());
}

fn user(json: &str) -> LeagueUser {
    serde_json::from_str(json).unwrap()
}

#[test]
fn league_user_label_prefers_custom_team_name() {
    let u = user(
        r#"{"user_id": "u1", "display_name": "handle",
            "metadata": {"team_name": "The Juggernauts"}}"#,
    );
    assert_eq!(u.label().as_deref(), Some("The Juggernauts"));
}

#[test]
fn league_user_label_falls_back_past_blank_team_names() {
    let blank =
        user(r#"{"user_id": "u1", "display_name": "handle", "metadata": {"team_name": "   "}}"#);
    assert_eq!(blank.label().as_deref(), Some("handle"));
    let no_meta = user(r#"{"user_id": "u1", "display_name": "handle"}"#);
    assert_eq!(no_meta.label().as_deref(), Some("handle"));
    let nothing = user(r#"{"user_id": "u1"}"#);
    assert!(nothing.label().is_none());
}

#[test]
fn league_user_avatar_prefers_custom_team_image() {
    let u = user(
        r#"{"user_id": "u1", "avatar": "abc123",
            "metadata": {"avatar": "https://sleepercdn.com/uploads/custom.jpg"}}"#,
    );
    assert_eq!(
        u.avatar_ref().as_deref(),
        Some("https://sleepercdn.com/uploads/custom.jpg")
    );
}

#[test]
fn league_user_avatar_falls_back_to_account_hash_or_nothing() {
    let account_only = user(r#"{"user_id": "u1", "avatar": "abc123", "metadata": {}}"#);
    assert_eq!(account_only.avatar_ref().as_deref(), Some("abc123"));
    let blank_custom = user(r#"{"user_id": "u1", "avatar": "abc123", "metadata": {"avatar": ""}}"#);
    assert_eq!(blank_custom.avatar_ref().as_deref(), Some("abc123"));
    let egg = user(r#"{"user_id": "u1", "avatar": null}"#);
    assert!(egg.avatar_ref().is_none());
    let blank_everything = user(r#"{"user_id": "u1", "avatar": "  ", "metadata": {"avatar": ""}}"#);
    assert!(blank_everything.avatar_ref().is_none());
}

#[test]
fn client_constructs_and_shares_its_pool_without_network() {
    let client = SleeperClient::new();
    let _pool = client.http_client();
    let _default = SleeperClient::default();
}
