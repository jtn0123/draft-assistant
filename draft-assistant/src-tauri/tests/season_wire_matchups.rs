//! The current week's matchup rows: what the load does when that one request
//! is lost.
//!
//! Its own binary rather than more of `tests/season_wire.rs`, which is near
//! the line cap and whose stub is shared by tests running in parallel: the
//! outage here has to be switched on for one league without the others
//! noticing, so each case gets a league id of its own.
//!
//! The bug all of this pins: with no rows for the current week nobody has an
//! opponent, `opponent_of` answers `None`, and the season screen printed
//! "Week 3, bye" over a week that was being played. Nothing on disk was
//! consulted and the source was stamped green on the way through.

mod stub;

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::season_engine::SeasonLoader;
use draft_assistant_lib::sleeper::League;

const WEEK: u32 = 3;
const STATE: &str = r#"{"season": "2026", "week": 3, "display_week": 3, "season_type": "regular"}"#;
const ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "user-a", "players": ["qb-1"], "starters": ["qb-1"],
     "settings": {"wins": 2, "losses": 1, "fpts": 310}},
    {"roster_id": 2, "owner_id": "user-b", "players": ["wr-1"], "starters": ["wr-1"],
     "settings": {"wins": 1, "losses": 2, "fpts": 288}}
]"#;
const PAIRED: &str = r#"[
    {"roster_id": 1, "matchup_id": 1, "points": 30.5, "players_points": {"qb-1": 3.0}},
    {"roster_id": 2, "matchup_id": 1, "points": 30.0, "players_points": {"wr-1": 3.0}}
]"#;

/// Flipped by the outage test, and only ever read for `league-flaky`.
static WEEK_IS_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How many times the current week was asked for under `league-once`, which
/// no other test in this binary loads.
static WEEK_HITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn league(id: &str) -> League {
    serde_json::from_str(&format!(
        r#"{{"league_id": "{id}", "name": "Season League", "season": "2026",
             "status": "in_season", "total_rosters": 2,
             "roster_positions": ["QB", "WR", "BN"],
             "scoring_settings": {{"rec": 1.0}}, "draft_id": "draft-1",
             "previous_league_id": null, "settings": {{"playoff_week_start": 15}}}}"#
    ))
    .expect("the fixture league must parse")
}

fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: &str| Some((200u16, body.to_string()));
    if path == "/v1/state/nfl" {
        return ok(STATE);
    }
    if path.starts_with("/scores/nfl/regular/2026/") {
        return ok("[]");
    }
    // The league whose current week answers `null`, the lost-response shape
    // Sleeper serves now and then. Every other week answers normally, so the
    // sweep is healthy and only the week being played is missing.
    if let Some(rest) = path.strip_prefix("/v1/league/league-null/") {
        if rest == "matchups/3" {
            return ok("null");
        }
        return route(&format!("/v1/league/league-flaky/{rest}"));
    }
    // The counted league: the same routes, with a tally on the week being
    // played so a duplicate request cannot hide.
    if let Some(rest) = path.strip_prefix("/v1/league/league-once/") {
        if rest == "matchups/3" {
            WEEK_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        return route(&format!("/v1/league/league-flaky/{rest}"));
    }
    let rest = path.strip_prefix("/v1/league/league-flaky/")?;
    match rest.split_once('/') {
        Some(("matchups", week)) => {
            if week == "3" && WEEK_IS_DOWN.load(std::sync::atomic::Ordering::SeqCst) {
                return Some((500, "\"boom\"".to_string()));
            }
            ok(PAIRED)
        }
        Some(("transactions", _)) => ok("[]"),
        _ => match rest {
            "rosters" => ok(ROSTERS),
            "winners_bracket" => ok("[]"),
            _ => None,
        },
    }
}

fn engine(label: &str) -> Engine {
    stub::serve(route);
    Engine::new(stub::scratch_dir(label))
}

/// The failed-fetch half: the rows are on disk from the load half an hour ago
/// and the load has to reach for them rather than showing a week with nobody
/// in it. The badge has to say they came off the disk, because a matchup list
/// served from a cache is not a live feed.
#[tokio::test]
async fn a_lost_current_week_falls_back_to_the_last_rows_on_disk() {
    let engine = engine("week-outage");
    let league = league("league-flaky");

    let first = engine
        .load_season(&league, Some("user-a"), true)
        .await
        .expect("the first load has every endpoint");
    assert_eq!(first.matchups.len(), 2);
    assert!(first.sources.matchups.error.is_none());

    WEEK_IS_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    let stale = engine.load_season(&league, Some("user-a"), true).await;
    WEEK_IS_DOWN.store(false, std::sync::atomic::Ordering::SeqCst);
    let stale = stale.expect("the cached rows must keep the matchup on screen");

    assert_eq!(
        stale.matchups.len(),
        2,
        "the cached matchup rows were not used, so the screen reads as a bye"
    );
    assert!(
        stale
            .warnings
            .iter()
            .any(|w| w.contains("this week's matchups could not be refreshed")),
        "the fallback has to admit itself: {:?}",
        stale.warnings
    );
    assert!(
        stale.sources.matchups.error.is_some(),
        "matchup rows served off disk must not be stamped as a live source"
    );
    std::fs::remove_dir_all(&engine.data_dir).ok();
}

/// The `null` half: an empty answer for the week being played is a lost
/// response, not a schedule with nobody in it. A bye week still has a row per
/// roster, with no `matchup_id` on it, so no rows at all can never be one.
/// With nothing on disk to fall back to the screen has to hear that the week
/// is unknown rather than be handed a green badge over an empty list.
#[tokio::test]
async fn an_empty_current_week_is_refused_rather_than_stamped_green() {
    let engine = engine("week-null");
    let season = engine
        .load_season(&league("league-null"), Some("user-a"), true)
        .await
        .expect("the rest of the load still runs");

    assert!(season.matchups.is_empty(), "there was nothing to show");
    assert_eq!(
        season.week, WEEK,
        "the week itself was never in doubt, only its rows"
    );
    assert!(
        season
            .sources
            .matchups
            .error
            .as_deref()
            .is_some_and(|e| e.contains("this week's matchups unavailable")),
        "an empty answer must fail the source: {:?}",
        season.sources.matchups.error
    );
    assert!(
        season
            .warnings
            .iter()
            .any(|w| w.contains("this week's matchups unavailable")),
        "{:?}",
        season.warnings
    );
    // Nothing empty was written down, so the next load asks again rather than
    // reading a blank file back.
    assert!(
        !engine
            .data_dir
            .join("season_league-null_week3.json")
            .exists(),
        "an empty current week was cached"
    );
    std::fs::remove_dir_all(&engine.data_dir).ok();
}

/// The duplicate request: the sweep's range covers the week being played, so
/// the load used to ask for it twice at once, and the two answers were free to
/// disagree. One request now, and the rows the header is built from are the
/// same ones the sweep totals.
#[tokio::test]
async fn the_current_week_is_asked_for_once_and_still_reaches_the_sweep() {
    let engine = engine("week-once");
    let season = engine
        .load_season(&league("league-once"), Some("user-a"), true)
        .await
        .expect("loaded");

    assert_eq!(
        WEEK_HITS.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the week being played was requested more than once in one load"
    );
    let swept: Vec<u32> = season.schedule.iter().map(|(w, _)| *w).collect();
    assert!(
        swept.contains(&WEEK),
        "the current week must still have pairings: {swept:?}"
    );
    // Weeks 1..3 are played and each gives qb-1 three points; weeks 4 and on
    // have not been played and must not count.
    assert_eq!(season.season_points.get("qb-1"), Some(&9.0));
    std::fs::remove_dir_all(&engine.data_dir).ok();
}
