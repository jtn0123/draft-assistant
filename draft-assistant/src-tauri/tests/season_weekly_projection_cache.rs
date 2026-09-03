//! What a half-answered weekly-projection sweep leaves on disk.
//!
//! Eighteen week requests go out and any one of them can fail. The result is
//! still usable — a missing week costs bonus precision, not correctness — but
//! it must not be written back to `weekly_{season}.json` with a fresh
//! timestamp, or the hole in it is served for the whole six-hour TTL and every
//! player in the missing week reads as unprojected. This drives a real load
//! against a stub that fails exactly one week; see `tests/stub/mod.rs`.

mod stub;

use draft_assistant_lib::engine::Engine;

/// The week whose request the stub refuses.
const TORN_WEEK: u32 = 3;

/// What the cache already holds for the torn week, from an earlier good fetch.
const CACHED_AT: u64 = 1_000;

const LEAGUE: &str = r#"{
    "league_id": "league-1", "name": "Torn Weeks", "season": "2026",
    "status": "drafting", "total_rosters": 2,
    "roster_positions": ["QB", "RB", "BN"],
    "scoring_settings": {"pass_yd": 0.04, "pass_td": 4.0, "rush_yd": 0.1},
    "draft_id": "draft-1"
}"#;

const PLAYERS: &str = r#"{
    "qb-1": {"full_name": "Wire Passer", "position": "QB", "team": "AAA"},
    "rb-1": {"full_name": "Wire Runner", "position": "RB", "team": "BBB"}
}"#;

const SEASON_ROWS: &str = r#"[
    {"player_id": "qb-1", "stats": {"pass_yd": 4200.0, "adp_ppr": 14.0},
     "player": {"position": "QB", "team": "AAA"}},
    {"player_id": "rb-1", "stats": {"rush_yd": 1200.0, "adp_ppr": 3.0},
     "player": {"position": "RB", "team": "BBB"}}
]"#;

fn weekly_rows(week: u32) -> String {
    format!(
        r#"[
        {{"player_id": "qb-1", "week": {week}, "stats": {{"pass_yd": 250.0}},
          "player": {{"position": "QB", "team": "AAA"}}}},
        {{"player_id": "rb-1", "week": {week}, "stats": {{"rush_yd": 70.0}},
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
            // The one week that will not answer.
            Some(week) if week.parse::<u32>() == Ok(TORN_WEEK) => {
                Some((503, "\"unavailable\"".to_string()))
            }
            Some(week) => ok(weekly_rows(week.parse().unwrap_or(1))),
            None => ok(SEASON_ROWS.to_string()),
        };
    }
    match path {
        "/v1/league/league-1" => ok(LEAGUE.to_string()),
        "/v1/league/league-1/users" => ok("[]".to_string()),
        "/v1/draft/draft-1" => ok(r#"{"draft_id": "draft-1", "status": "drafting",
            "type": "snake", "settings": {"teams": 2, "rounds": 3},
            "season": "2026"}"#
            .to_string()),
        "/v1/draft/draft-1/picks" | "/v1/draft/draft-1/traded_picks" => ok("[]".to_string()),
        _ => None,
    }
}

fn engine(label: &str) -> Engine {
    stub::serve(route);
    Engine::new(stub::scratch_dir(label))
}

/// Put a good copy of the torn week on disk, stamped long ago.
fn seed_cache(engine: &Engine) {
    let cached = format!(
        r#"{{"fetched_at": {CACHED_AT}, "data": [
            {{"player_id": "qb-1", "week": {TORN_WEEK}, "stats": {{"pass_yd": 300.0}}}}
        ]}}"#
    );
    std::fs::write(engine.data_dir.join("weekly_2026.json"), cached).expect("seed the cache");
}

fn cached_fetched_at(engine: &Engine) -> u64 {
    let raw = std::fs::read_to_string(engine.data_dir.join("weekly_2026.json"))
        .expect("the cache file is still there");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("the cache file parses");
    parsed["fetched_at"].as_u64().expect("a timestamp")
}

#[tokio::test]
async fn a_half_answered_sweep_is_not_stamped_fresh_over_the_good_copy() {
    let engine = engine("weekly-partial");
    seed_cache(&engine);

    let loaded = engine
        .load_league("league-1", true)
        .await
        .expect("one bad week must not fail the load");

    // The load says which week is missing, as it always did.
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w.contains("weekly projections unavailable for weeks 3")),
        "{:?}",
        loaded.warnings
    );

    // The good copy is untouched: a partial sweep written back with today's
    // timestamp would have served its hole for the whole TTL.
    assert_eq!(
        cached_fetched_at(&engine),
        CACHED_AT,
        "a partial sweep must not overwrite the cache"
    );

    // The torn week is filled in from that copy rather than left empty, so
    // nobody in week 3 reads as unprojected. 300 pass yards at 0.04.
    assert!(
        loaded.weekly_points.has_week(TORN_WEEK),
        "the stale week should have been merged in"
    );
    let points = loaded
        .weekly_points
        .get("qb-1", TORN_WEEK)
        .expect("the cached week-3 row");
    assert!((points - 12.0).abs() < 1e-9, "week 3 scored {points}");

    // The weeks that did answer are this run's, not the cache's.
    let week_one = loaded
        .weekly_points
        .get("qb-1", 1)
        .expect("week 1 answered");
    assert!((week_one - 10.0).abs() < 1e-9, "week 1 scored {week_one}");

    std::fs::remove_dir_all(&engine.data_dir).ok();
}

/// With nothing on disk to fall back on, the sweep still returns what it got —
/// it just does not leave a file behind claiming to be a full one.
#[tokio::test]
async fn a_half_answered_sweep_with_no_cache_writes_no_cache() {
    let engine = engine("weekly-partial-bare");
    let loaded = engine
        .load_league("league-1", true)
        .await
        .expect("one bad week must not fail the load");

    assert!(
        !loaded.weekly_points.has_week(TORN_WEEK),
        "there was nothing to fill week 3 in from"
    );
    assert!(loaded.weekly_points.has_week(1));
    assert!(
        !engine.data_dir.join("weekly_2026.json").exists(),
        "a partial sweep must not be cached at all"
    );

    std::fs::remove_dir_all(&engine.data_dir).ok();
}
