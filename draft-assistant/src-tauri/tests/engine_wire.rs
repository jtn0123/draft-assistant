//! Draft loading end to end against a stub Sleeper.
//!
//! `tests/engine_offline.rs` covers what happens when nothing answers. This
//! is the other half: every endpoint answers, and the assembled
//! `LoadedLeague` is checked against what the screen shows — board names,
//! manager labels, traded picks, and the warnings that explain a partial
//! load. Nothing here touches the network; see `tests/stub/mod.rs`.

mod stub;

use draft_assistant_lib::engine::Engine;

const LEAGUE: &str = r#"{
    "league_id": "league-1", "name": "Wire League", "season": "2026",
    "status": "drafting", "total_rosters": 2,
    "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
    "scoring_settings": {"pass_yd": 0.04, "pass_td": 4.0, "rush_yd": 0.1,
                         "rush_td": 6.0, "rec_yd": 0.1, "rec_td": 6.0, "rec": 1.0},
    "draft_id": "draft-1"
}"#;

/// Same league, but its draft's trade list is broken.
const LEAGUE_TORN: &str = r#"{
    "league_id": "league-torn", "name": "Torn League", "season": "2026",
    "status": "drafting", "total_rosters": 2,
    "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
    "scoring_settings": {"rec": 1.0},
    "draft_id": "draft-torn"
}"#;

/// A league whose draft has never been configured: no teams, no rounds.
const LEAGUE_UNSET: &str = r#"{
    "league_id": "league-unset", "name": "Unset League", "season": "2026",
    "status": "pre_draft", "total_rosters": 0,
    "roster_positions": ["QB", "BN"], "scoring_settings": {},
    "draft_id": "draft-unset"
}"#;

fn draft_json(id: &str) -> String {
    format!(
        r#"{{"draft_id": "{id}", "status": "drafting", "type": "snake",
            "settings": {{"teams": 2, "rounds": 6}},
            "draft_order": {{"user-a": 1, "user-b": 2}},
            "slot_to_roster_id": {{"1": 1, "2": 2}},
            "season": "2026"}}"#
    )
}

const MOCK_DRAFT: &str = r#"{
    "draft_id": "mock-1", "status": "pre_draft", "type": "snake",
    "settings": {"teams": 12, "rounds": 5, "slots_qb": 1, "slots_rb": 2,
                 "slots_wr": 2, "slots_flex": 1},
    "metadata": {"scoring_type": "half_ppr"},
    "season": "2026"
}"#;

const USERS: &str = r#"[
    {"user_id": "user-a", "display_name": "Ada", "metadata": {"team_name": "Ada's Autos"}},
    {"user_id": "user-b", "display_name": "Bo"}
]"#;

const TRADED: &str = r#"[
    {"season": "2026", "round": 2, "roster_id": 1, "previous_owner_id": 1, "owner_id": 2}
]"#;

const PLAYERS: &str = r#"{
    "qb-1": {"full_name": "Wire Passer", "position": "QB", "team": "AAA"},
    "rb-1": {"full_name": "Wire Runner", "position": "RB", "team": "BBB"},
    "wr-1": {"full_name": "Wire Catcher", "position": "WR", "team": "CCC"},
    "te-1": {"full_name": "Wire Tight", "position": "TE", "team": "DDD"}
}"#;

const SEASON_ROWS: &str = r#"[
    {"player_id": "qb-1", "stats": {"pass_yd": 4200.0, "pass_td": 32.0, "adp_ppr": 14.0},
     "player": {"position": "QB", "team": "AAA"}},
    {"player_id": "rb-1", "stats": {"rush_yd": 1200.0, "rush_td": 10.0, "rec": 40.0, "adp_ppr": 3.0},
     "player": {"position": "RB", "team": "BBB"}},
    {"player_id": "wr-1", "stats": {"rec_yd": 1300.0, "rec_td": 9.0, "rec": 95.0, "adp_ppr": 5.0},
     "player": {"position": "WR", "team": "CCC"}},
    {"player_id": "te-1", "stats": {"rec_yd": 800.0, "rec_td": 6.0, "rec": 70.0, "adp_ppr": 30.0},
     "player": {"position": "TE", "team": "DDD"}}
]"#;

/// One week of per-player rows. Team AAA is missing an opponent, which is how
/// Sleeper spells a bye.
fn weekly_rows(week: u32) -> String {
    let aaa_opponent = if week == 7 { "null" } else { "\"BBB\"" };
    format!(
        r#"[
        {{"player_id": "qb-1", "week": {week}, "opponent": {aaa_opponent},
          "stats": {{"pass_yd": 250.0, "pass_td": 2.0}},
          "player": {{"position": "QB", "team": "AAA"}}}},
        {{"player_id": "rb-1", "week": {week}, "opponent": "AAA",
          "stats": {{"rush_yd": 70.0, "rush_td": 0.6}},
          "player": {{"position": "RB", "team": "BBB"}}}}
    ]"#
    )
}

fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: String| Some((200u16, body));
    if path == "/v1/players/nfl" {
        return ok(PLAYERS.to_string());
    }
    if let Some(rest) = path.strip_prefix("/projections/nfl/2026") {
        return match rest.strip_prefix('/') {
            Some(week) => ok(weekly_rows(week.parse().unwrap_or(1))),
            None => ok(SEASON_ROWS.to_string()),
        };
    }
    match path {
        "/v1/league/league-1" => ok(LEAGUE.to_string()),
        "/v1/league/league-torn" => ok(LEAGUE_TORN.to_string()),
        "/v1/league/league-unset" => ok(LEAGUE_UNSET.to_string()),
        "/v1/league/league-1/users" | "/v1/league/league-torn/users" => ok(USERS.to_string()),
        "/v1/league/league-unset/users" => ok("[]".to_string()),
        "/v1/draft/draft-1" | "/v1/draft/draft-torn" => {
            ok(draft_json(path.rsplit('/').next().unwrap_or("draft-1")))
        }
        "/v1/draft/draft-unset" => ok(r#"{"draft_id": "draft-unset", "status": "pre_draft",
            "type": "snake", "settings": {"teams": 0, "rounds": 0}, "season": "2026"}"#
            .to_string()),
        "/v1/draft/mock-1" => ok(MOCK_DRAFT.to_string()),
        "/v1/draft/draft-1/picks"
        | "/v1/draft/draft-torn/picks"
        | "/v1/draft/draft-unset/picks"
        | "/v1/draft/mock-1/picks" => ok("[]".to_string()),
        "/v1/draft/draft-1/traded_picks" => ok(TRADED.to_string()),
        "/v1/draft/mock-1/traded_picks" | "/v1/draft/draft-unset/traded_picks" => {
            ok("[]".to_string())
        }
        // The one endpoint that is deliberately broken.
        "/v1/draft/draft-torn/traded_picks" => Some((503, "\"unavailable\"".to_string())),
        _ => None,
    }
}

fn engine(label: &str) -> Engine {
    stub::serve(route);
    Engine::new(stub::scratch_dir(label))
}

fn cleanup(engine: Engine) {
    std::fs::remove_dir_all(&engine.data_dir).ok();
}

#[tokio::test]
async fn a_league_load_assembles_a_named_scored_board() {
    let engine = engine("league");
    let loaded = engine
        .load_league("league-1", true)
        .await
        .expect("every endpoint answered");

    assert_eq!(loaded.league.name, "Wire League");
    // Names come from the players dictionary, not the projection rows, which
    // carry no name at all — a board of bare player ids is the bug this
    // catches.
    let names: Vec<&str> = loaded.board.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Wire Runner"), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("rb-")), "{names:?}");
    // The board is sorted by value, and the index agrees with it.
    for (i, player) in loaded.board.iter().enumerate() {
        assert_eq!(loaded.board_index.get(&player.player_id), Some(&i));
    }
    // A custom team name wins over the display name; a manager without one
    // keeps theirs.
    assert_eq!(
        loaded.user_names.get("user-a").map(String::as_str),
        Some("Ada's Autos")
    );
    assert_eq!(
        loaded.user_names.get("user-b").map(String::as_str),
        Some("Bo")
    );
    assert_eq!(loaded.traded_picks.len(), 1);
    assert_eq!(loaded.traded_picks[0].owner_id, 2);
    // The first pick fetch worked, so the poll starts out healthy.
    assert!(loaded.poll_last_success_at.is_some());
    assert_eq!(loaded.poll_consecutive_failures, 0);
    assert!(loaded.poll_last_error.is_none());
    cleanup(engine);
}

#[tokio::test]
async fn a_four_player_board_says_so_instead_of_looking_complete() {
    let engine = engine("small-board");
    let loaded = engine.load_league("league-1", true).await.expect("loaded");
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w.contains("board unusually small")),
        "{:?}",
        loaded.warnings
    );
    cleanup(engine);
}

#[tokio::test]
async fn a_bye_week_is_inferred_from_the_week_with_no_opponent() {
    let engine = engine("bye");
    let loaded = engine.load_league("league-1", true).await.expect("loaded");
    let passer = loaded
        .board
        .iter()
        .find(|p| p.player_id == "qb-1")
        .expect("the passer is on the board");
    assert_eq!(passer.bye_week, Some(7));
    cleanup(engine);
}

#[tokio::test]
async fn a_trade_list_outage_costs_the_trades_not_the_draft() {
    let engine = engine("torn");
    let loaded = engine
        .load_league("league-torn", true)
        .await
        .expect("a broken trade list must not fail the load");
    assert!(loaded.traded_picks.is_empty());
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w.contains("pick order shown as a plain snake")),
        "{:?}",
        loaded.warnings
    );
    cleanup(engine);
}

#[tokio::test]
async fn a_mock_draft_gets_its_league_settings_synthesized() {
    let engine = engine("mock");
    let loaded = engine
        .load_draft_only("mock-1", true)
        .await
        .expect("a bare draft id must load");

    // Five rounds against six declared starters: the roster is the starters,
    // with no bench invented for rounds that do not exist.
    assert_eq!(
        loaded.league.roster_positions,
        vec!["QB", "RB", "RB", "WR", "WR", "FLEX"]
    );
    assert_eq!(loaded.league.total_rosters, 12);
    // half_ppr in the draft metadata has to reach the scoring table, or every
    // receiver on the synthesized board is mis-valued.
    assert_eq!(loaded.league.scoring_settings.get("rec"), Some(&0.5));
    assert_eq!(loaded.league.name, "Mock draft (half_ppr)");
    assert!(
        loaded.warnings.iter().any(|w| w.contains("mock draft")),
        "{:?}",
        loaded.warnings
    );
    cleanup(engine);
}

#[tokio::test]
async fn a_draft_with_no_teams_is_refused_by_name() {
    let engine = engine("unset");
    let err = engine
        .load_league("league-unset", true)
        .await
        .err()
        .expect("a draft with no teams cannot be laid out");
    assert!(err.contains("draft-unset"), "{err}");
    assert!(err.contains("has not been set up yet"), "{err}");
    cleanup(engine);
}

#[tokio::test]
async fn an_id_that_is_not_a_league_is_tried_as_a_draft() {
    let engine = engine("load-any");
    let loaded = engine
        .load_any("mock-1", true)
        .await
        .expect("the id is a mock draft even though it is not a league");
    assert_eq!(loaded.draft.draft_id, "mock-1");
    cleanup(engine);
}

#[tokio::test]
async fn a_repeat_load_is_served_from_disk_rather_than_the_wire() {
    let engine = engine("cached");
    engine.load_league("league-1", true).await.expect("loaded");
    // The dictionary is cached in this engine's own data dir, so the proof is
    // that file: a fetch rewrites it, a cache hit leaves it alone. (One stub
    // serves every test in this binary, so a count of what it served could
    // not tell this engine's fetches from a neighbour's.)
    let cached = engine.data_dir.join("players.json");
    let written = std::fs::metadata(&cached)
        .and_then(|m| m.modified())
        .expect("the first load cached the players dictionary");
    engine
        .load_league("league-1", false)
        .await
        .expect("second load");
    let after = std::fs::metadata(&cached)
        .and_then(|m| m.modified())
        .expect("the cache is still there");
    assert_eq!(
        after, written,
        "the 15 MB players dictionary must not be re-fetched inside its TTL"
    );
    cleanup(engine);
}
