//! The season poller's half-hour player refresh, driven through the real
//! `impl PlayerRefresh for Engine` rather than a stub of it.
//!
//! Every other poller test replaces that trait wholesale, so the one argument
//! that makes the refresh a real one — the half-hour ceiling handed to
//! `players_no_older_than` — had nothing standing behind it: put the 24-hour
//! default back and the whole suite still passed, while a starter ruled Out
//! at noon sat in the optimal lineup for the rest of Sunday. This asks the
//! engine itself, over a stub that counts requests, and pins the ceiling from
//! both sides: a cache older than half an hour must be refetched, and a young
//! one must not be.

mod stub;

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::season_refresh::PlayerRefresh;
use std::sync::atomic::{AtomicUsize, Ordering};

const SEASON: u32 = 2026;
const DICTIONARY: &str = r#"{"p-fresh": {"full_name": "Fresh Off The Wire", "position": "RB"}}"#;

static PLAYER_HITS: AtomicUsize = AtomicUsize::new(0);
static WEEKLY_HITS: AtomicUsize = AtomicUsize::new(0);

fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    if path == "/v1/players/nfl" {
        PLAYER_HITS.fetch_add(1, Ordering::SeqCst);
        return Some((200, DICTIONARY.to_string()));
    }
    if path.starts_with("/projections/nfl/2026/") {
        WEEKLY_HITS.fetch_add(1, Ordering::SeqCst);
        return Some((200, "[]".to_string()));
    }
    None
}

/// Write both caches this refresh reads, stamped `age` seconds ago.
fn aged_caches(engine: &Engine, age: u64) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs()
        - age;
    std::fs::create_dir_all(&engine.data_dir).expect("the data directory");
    let files = [
        (
            "players.json",
            r#"{"p-cached": {"full_name": "From The Disk", "position": "RB"}}"#,
        ),
        ("weekly_2026.json", "[]"),
    ];
    for (name, data) in files {
        std::fs::write(
            engine.data_dir.join(name),
            format!(r#"{{"fetched_at": {at}, "data": {data}}}"#),
        )
        .expect("write a cache envelope");
    }
}

/// One test rather than two: the request counters are per process and the
/// players endpoint has no per-league door to hide behind, so two tests of
/// this would race each other.
#[tokio::test]
async fn the_half_hour_ceiling_is_the_one_the_poller_actually_asks_with() {
    stub::serve(route);
    let engine = Engine::new(stub::scratch_dir("players-refresh"));

    // Thirty-one minutes old: inside the 24-hour default the dictionary is
    // fetched with everywhere else, and outside the poller's own ceiling.
    aged_caches(&engine, 31 * 60);
    let refreshed = engine
        .refresh_players(SEASON)
        .await
        .expect("the stub answered both halves");
    assert_eq!(
        PLAYER_HITS.load(Ordering::SeqCst),
        1,
        "a 31-minute dictionary must go back to the network, or a Saturday \
         night Out never reaches Sunday's lineup"
    );
    assert!(
        WEEKLY_HITS.load(Ordering::SeqCst) > 0,
        "the weekly projections are on the same ceiling"
    );
    assert!(
        refreshed.staleness().is_none(),
        "everything answered, so nothing is stale: {:?}",
        refreshed.staleness()
    );

    // Five minutes old: the ceiling is a ceiling, not a cache bypass. Asking
    // with no ceiling at all would re-download ~14.6 MB every half hour for
    // an answer already on disk.
    let before = (
        PLAYER_HITS.load(Ordering::SeqCst),
        WEEKLY_HITS.load(Ordering::SeqCst),
    );
    aged_caches(&engine, 5 * 60);
    engine
        .refresh_players(SEASON)
        .await
        .expect("served from the two caches");
    assert_eq!(
        (
            PLAYER_HITS.load(Ordering::SeqCst),
            WEEKLY_HITS.load(Ordering::SeqCst)
        ),
        before,
        "a five-minute cache must be served as it is"
    );

    std::fs::remove_dir_all(&engine.data_dir).ok();
}

/// The staleness warning the refresh carries when `/players` is down and the
/// copy on disk is what comes back instead. It used to be dropped at the call
/// site, which is what made a dead dictionary invisible: see
/// `tests/season_poll_upkeep.rs` for the badge that now shows it.
#[tokio::test]
async fn a_dictionary_served_off_the_disk_says_so() {
    // A host nothing is listening on: every request is refused at once.
    let engine = Engine::with_client(
        stub::scratch_dir("players-offline"),
        draft_assistant_lib::sleeper::SleeperClient::with_host("http://127.0.0.1:1"),
    );
    aged_caches(&engine, 31 * 60);

    let refreshed = engine
        .refresh_players(SEASON)
        .await
        .expect("the copies on disk are still worth applying");

    let note = refreshed
        .staleness()
        .expect("a fallback to disk has to admit itself");
    assert!(note.contains("players refresh failed"), "{note}");
    std::fs::remove_dir_all(&engine.data_dir).ok();
}
