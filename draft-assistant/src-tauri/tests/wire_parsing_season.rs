//! Deserialization tests for the in-season Sleeper wire types.

use draft_assistant_lib::season_api::{NflState, Roster, Transaction};

#[test]
fn nfl_state_current_week_prefers_display_week() {
    let state: NflState = serde_json::from_str(
        r#"{"week": 3, "display_week": 4, "season": "2025",
            "season_type": "regular", "previous_season": "2024"}"#,
    )
    .unwrap();
    assert_eq!(state.current_week(), 4);
    assert_eq!(state.season, "2025");
    assert_eq!(state.previous_season.as_deref(), Some("2024"));
}

#[test]
fn nfl_state_defaults_and_clamps_to_week_one() {
    let empty: NflState = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.current_week(), 1, "preseason week 0 clamps to 1");
    assert_eq!(empty.season, "");
    let no_display: NflState = serde_json::from_str(r#"{"week": 7}"#).unwrap();
    assert_eq!(no_display.current_week(), 7);
}

#[test]
fn roster_parses_split_point_totals() {
    let roster: Roster = serde_json::from_str(
        r#"{
            "roster_id": 3,
            "owner_id": "user-a",
            "players": ["4034", "6786"],
            "starters": ["4034"],
            "settings": {
                "wins": 8, "losses": 5, "ties": 1,
                "fpts": 1543, "fpts_decimal": 22,
                "fpts_against": 1499, "fpts_against_decimal": 80,
                "waiver_budget_used": 37, "waiver_position": 9, "total_moves": 14
            }
        }"#,
    )
    .unwrap();
    assert_eq!(roster.roster_id, 3);
    assert_eq!(roster.owner_id.as_deref(), Some("user-a"));
    assert_eq!(roster.player_ids(), ["4034".to_string(), "6786".into()]);
    assert_eq!(roster.starter_ids(), ["4034".to_string()]);
    assert_eq!(roster.settings.wins, 8);
    assert!((roster.settings.points_for() - 1543.22).abs() < 1e-9);
    assert!((roster.settings.points_against() - 1499.80).abs() < 1e-9);
    assert_eq!(roster.settings.waiver_position, Some(9));
}

#[test]
fn roster_minimal_payload_defaults_everything() {
    let roster: Roster = serde_json::from_str(r#"{"roster_id": 1}"#).unwrap();
    assert!(roster.owner_id.is_none());
    assert!(roster.player_ids().is_empty());
    assert!(roster.starter_ids().is_empty());
    assert_eq!(roster.settings.wins, 0);
    assert_eq!(roster.settings.points_for(), 0.0);
}

#[test]
fn transactions_parse_waiver_claims_and_trades() {
    let json = r#"[
        {
            "transaction_id": "t-1",
            "type": "waiver",
            "status": "complete",
            "created": 1756400000000,
            "adds": {"4034": 3},
            "drops": {"6786": 3},
            "roster_ids": [3],
            "settings": {"waiver_bid": 17}
        },
        {"transaction_id": "t-2", "type": "trade"}
    ]"#;
    let txs: Vec<Transaction> = serde_json::from_str(json).unwrap();
    assert_eq!(txs.len(), 2);
    let waiver = &txs[0];
    assert_eq!(waiver.transaction_id, "t-1");
    assert_eq!(waiver.kind, "waiver", "wire field is `type`");
    assert_eq!(waiver.status, "complete");
    assert_eq!(waiver.created, 1_756_400_000_000);
    assert_eq!(waiver.adds.as_ref().unwrap().get("4034"), Some(&3));
    assert_eq!(waiver.drops.as_ref().unwrap().get("6786"), Some(&3));
    assert_eq!(waiver.roster_ids, [3]);
    assert_eq!(waiver.settings.as_ref().unwrap().waiver_bid, Some(17));
    let trade = &txs[1];
    assert_eq!(trade.kind, "trade");
    assert_eq!(trade.status, "");
    assert_eq!(trade.created, 0);
    assert!(trade.adds.is_none());
    assert!(trade.settings.is_none());
}
