//! In-season loading end to end against a stub Sleeper.
//!
//! `tests/engine_offline.rs` covers the total-outage path. This covers the
//! one the season screen actually runs on: state, rosters, this week's
//! matchups, the NFL scoreboard, two weeks of transactions, the full-season
//! matchup sweep, and last season's final table from the previous league in
//! the chain. Nothing here touches the network; see `tests/stub/mod.rs`.

mod stub;

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::season_engine::SeasonLoader;
use draft_assistant_lib::sleeper::League;

const CURRENT_WEEK: u32 = 3;

/// A league in its third week, following on from `league-2025`.
fn league(id: &str, previous: Option<&str>) -> League {
    let previous = match previous {
        Some(p) => format!("\"{p}\""),
        None => "null".to_string(),
    };
    serde_json::from_str(&format!(
        r#"{{"league_id": "{id}", "name": "Season League", "season": "2026",
             "status": "in_season", "total_rosters": 2,
             "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
             "scoring_settings": {{"rec": 1.0}},
             "draft_id": "draft-1",
             "previous_league_id": {previous},
             "settings": {{"playoff_week_start": 15}}}}"#
    ))
    .expect("the fixture league must parse")
}

const STATE: &str = r#"{"season": "2026", "week": 3, "display_week": 3, "season_type": "regular"}"#;

/// Flipped by the outage test so `/v1/state/nfl` — and only that route —
/// starts failing. The stub's router is a plain `fn`, so the switch has to
/// live somewhere it can reach.
static STATE_IS_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The stub is shared by every test in this binary and they run in parallel,
/// so a load that happens to overlap the outage would see the 500 too. Every
/// ordinary test holds this for reading while it loads; the outage test takes
/// it for writing around the flip, and so waits for the others to finish.
static STATE_GATE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());

const ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "user-a", "players": ["qb-1", "rb-1"],
     "starters": ["qb-1"], "settings": {"wins": 2, "losses": 1, "fpts": 310, "fpts_decimal": 50}},
    {"roster_id": 2, "owner_id": "user-b", "players": ["wr-1"],
     "starters": ["wr-1"], "settings": {"wins": 1, "losses": 2, "fpts": 288, "fpts_decimal": 25}}
]"#;

fn matchups(week: u32) -> String {
    format!(
        r#"[
        {{"roster_id": 1, "matchup_id": 1, "points": {week}0.5,
          "players_points": {{"qb-1": {week}.0, "rb-1": 2.0}}}},
        {{"roster_id": 2, "matchup_id": 1, "points": {week}0.0,
          "players_points": {{"wr-1": 3.0}}}}
    ]"#
    )
}

const SCORES: &str = r#"[
    {"status": "in_game", "metadata": {"home_team": "AAA", "away_team": "BBB",
     "home_score": 14, "away_score": "7", "quarter_num": 2}}
]"#;

fn transactions(week: u32) -> String {
    format!(
        r#"[
        {{"transaction_id": "t{week}", "type": "waiver", "status": "complete",
          "status_updated": {week}000, "roster_ids": [1],
          "adds": {{"te-1": 1}}, "drops": null,
          "settings": {{"waiver_bid": {week}}}}}
    ]"#
    )
}

/// Last season's rosters. Roster 2 scored the most points; roster 1 won the
/// title game, so it must still be listed first.
const LAST_ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "user-a", "settings": {"wins": 8, "losses": 6, "fpts": 1500}},
    {"roster_id": 2, "owner_id": "user-b", "settings": {"wins": 11, "losses": 3, "fpts": 1800}},
    {"roster_id": 3, "owner_id": "user-gone", "settings": {"wins": 1, "losses": 12, "ties": 1, "fpts": 900}}
]"#;

const LAST_USERS: &str = r#"[
    {"user_id": "user-a", "display_name": "Ada"},
    {"user_id": "user-b", "display_name": "Bo"}
]"#;

const LAST_BRACKET: &str = r#"[{"r": 3, "p": 1, "w": 1, "l": 2}]"#;

fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: String| Some((200u16, body));
    if path == "/v1/state/nfl" {
        if STATE_IS_DOWN.load(std::sync::atomic::Ordering::SeqCst) {
            return Some((500, "\"boom\"".to_string()));
        }
        return ok(STATE.to_string());
    }
    if path.starts_with("/scores/nfl/regular/2026/") {
        return ok(SCORES.to_string());
    }
    if let Some(rest) = path.strip_prefix("/v1/league/league-2026/") {
        return match rest {
            "rosters" => ok(ROSTERS.to_string()),
            "winners_bracket" => ok("[]".to_string()),
            _ => match rest.split_once('/') {
                // Week 2 has no matchup rows at all, which is how the sweep's
                // "unavailable" warning gets exercised.
                Some(("matchups", "2")) => Some((500, "\"boom\"".to_string())),
                Some(("matchups", week)) => ok(matchups(week.parse().unwrap_or(1))),
                Some(("transactions", week)) => ok(transactions(week.parse().unwrap_or(1))),
                _ => None,
            },
        };
    }
    match path {
        "/v1/league/league-2025/rosters" => ok(LAST_ROSTERS.to_string()),
        "/v1/league/league-2025/users" => ok(LAST_USERS.to_string()),
        "/v1/league/league-2025/winners_bracket" => ok(LAST_BRACKET.to_string()),
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
async fn a_season_load_gathers_the_week_the_table_and_the_scoreboard() {
    let engine = engine("season");
    let _state_up = STATE_GATE.read().await;
    let season = engine
        .load_season(&league("league-2026", None), Some("user-a"), true)
        .await
        .expect("every endpoint answered");

    assert_eq!(season.week, CURRENT_WEEK);
    assert_eq!(season.season, 2026);
    assert_eq!(season.rosters.len(), 2);
    // Sleeper splits points across two integer fields; a table showing 310
    // instead of 310.5 is the bug this pins.
    assert_eq!(season.rosters[0].settings.points_for(), 310.5);
    assert_eq!(season.matchups.len(), 2);
    // An away score arriving as a string must still be a number on the
    // scoreboard.
    assert_eq!(season.scores.len(), 1);
    assert_eq!(
        season.scores[0]
            .metadata
            .as_ref()
            .and_then(|m| m.away_score),
        Some(7)
    );
    // The feed spans this week and last, so both weeks' claims are in it.
    let ids: Vec<&str> = season
        .transactions
        .iter()
        .map(|t| t.transaction_id.as_str())
        .collect();
    assert!(ids.contains(&"t2"), "{ids:?}");
    assert!(ids.contains(&"t3"), "{ids:?}");
    // Every source answered on the first load, so the health badge is green
    // from the start rather than waiting for a refresh.
    assert!(season.sources.rosters.error.is_none());
    assert!(season.sources.matchups.error.is_none());
    assert!(season.sources.scores.error.is_none());
    cleanup(engine);
}

#[tokio::test]
async fn the_sweep_totals_only_the_weeks_already_played() {
    let engine = engine("sweep");
    let _state_up = STATE_GATE.read().await;
    let season = engine
        .load_season(&league("league-2026", None), None, true)
        .await
        .expect("loaded");

    // Weeks 1..14 are swept for pairings; week 2 is broken, so it is missing.
    let swept: Vec<u32> = season.schedule.iter().map(|(w, _)| *w).collect();
    assert!(swept.contains(&1) && swept.contains(&14), "{swept:?}");
    assert!(
        !swept.contains(&2),
        "a failed week has no pairings: {swept:?}"
    );
    assert!(
        season
            .warnings
            .iter()
            .any(|w| w.contains("matchups unavailable for week 2")),
        "{:?}",
        season.warnings
    );
    // Weeks 1 and 3 count toward the season total (1.0 + 3.0); week 4 onward
    // has not been played and must not, or every player looks like they have
    // already banked their whole season.
    assert_eq!(season.season_points.get("qb-1"), Some(&4.0));
    cleanup(engine);
}

#[tokio::test]
async fn last_seasons_table_puts_the_champion_above_the_points_leader() {
    let engine = engine("last-season");
    let _state_up = STATE_GATE.read().await;
    let season = engine
        .load_season(
            &league("league-2026", Some("league-2025")),
            Some("user-a"),
            true,
        )
        .await
        .expect("loaded");

    let table = &season.last_season;
    assert_eq!(table.len(), 3);
    assert_eq!(table[0].name, "Ada");
    assert_eq!(table[0].place, 1);
    assert_eq!(table[0].tag.as_deref(), Some("Champ"));
    assert!(table[0].is_mine, "the signed-in manager is marked");
    // Bo outscored everyone but lost the title game.
    assert_eq!(table[1].name, "Bo");
    assert_eq!(table[1].tag.as_deref(), Some("Most pts"));
    assert!(!table[1].is_mine);
    // A manager who has left the league keeps a usable label, and a tie shows
    // as three numbers rather than being silently dropped.
    assert_eq!(table[2].name, "Team 3");
    assert_eq!(table[2].record, "1\u{2013}12\u{2013}1");
    assert_eq!(table[2].tag, None);
    cleanup(engine);
}

#[tokio::test]
async fn a_league_with_no_previous_season_shows_no_table() {
    let engine = engine("no-previous");
    let _state_up = STATE_GATE.read().await;
    for previous in [None, Some(""), Some("0")] {
        let season = engine
            .load_season(&league("league-2026", previous), None, true)
            .await
            .expect("loaded");
        assert!(
            season.last_season.is_empty(),
            "{previous:?} is not a previous league"
        );
    }
    cleanup(engine);
}

#[tokio::test]
async fn a_live_refresh_moves_the_clock_and_the_scores() {
    let engine = engine("refresh");
    let _state_up = STATE_GATE.read().await;
    let mut season = engine
        .load_season(&league("league-2026", None), None, true)
        .await
        .expect("loaded");
    season.fetched_at = 0;
    season.scores.clear();

    engine
        .refresh_live(&mut season, "league-2026")
        .await
        .expect("every live endpoint answered");

    assert_eq!(season.scores.len(), 1, "the scoreboard came back");
    assert!(
        season.fetched_at > 0,
        "a successful refresh restarts the staleness clock"
    );
    assert_eq!(season.sources.scores.last_success_secs, season.fetched_at);
    cleanup(engine);
}

/// Which week it is is the first thing every other request needs, so losing
/// that one endpoint used to take the whole screen down — no rosters, no
/// matchups, no scoreboard, on data already sitting on disk. The last state
/// seen is now kept and used, with the usual admission that it is stale.
#[tokio::test]
async fn a_week_that_cannot_be_checked_falls_back_to_the_last_one_seen() {
    let engine = engine("state-outage");
    let league = league("league-2026", None);

    // One good load banks the state envelope.
    let first = engine
        .load_season(&league, Some("user-a"), true)
        .await
        .expect("the first load has every endpoint");
    assert_eq!(first.week, CURRENT_WEEK);
    assert!(
        !first
            .warnings
            .iter()
            .any(|w| w.contains("which NFL week it is")),
        "a load that checked the week must not claim to be stale: {:?}",
        first.warnings
    );

    let outage = STATE_GATE.write().await;
    STATE_IS_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    let stale = engine
        .load_season(&league, Some("user-a"), true)
        .await
        .expect("the cached week must keep the screen alive");
    STATE_IS_DOWN.store(false, std::sync::atomic::Ordering::SeqCst);
    drop(outage);

    assert_eq!(stale.week, CURRENT_WEEK);
    assert_eq!(stale.rosters.len(), 2, "the rest of the load still ran");
    assert!(
        stale
            .warnings
            .iter()
            .any(|w| w.contains("which NFL week it is could not be checked")),
        "the fallback has to admit itself: {:?}",
        stale.warnings
    );
    cleanup(engine);
}
