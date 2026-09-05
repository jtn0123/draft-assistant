//! What the engine keeps when Yahoo throttles a player-pool load partway.
//!
//! The pool is two dozen requests. Before this, a failure on any one of them
//! threw away every page already fetched and the next attempt started again
//! at page zero — straight back into the same throttle, and a board that
//! never loaded while the draft ran. These drive `Engine::yahoo_pool`
//! against the stub twice and assert on where the second walk starts.
//!
//! No Keychain, no network beyond a listener on 127.0.0.1, and the cache
//! directory is a scratch one this file deletes.

mod yahoo_stub;

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::engine_yahoo::cache_name;
use draft_assistant_lib::yahoo::{YahooClient, YahooHosts, PAGE};
use draft_assistant_lib::yahoo_oauth::{TokenSet, YahooCredentials};
use draft_assistant_lib::yahoo_pool::PlayerPool;
use draft_assistant_lib::yahoo_retry::RetryPolicy;
use draft_assistant_lib::yahoo_types::YahooPlayer;
use yahoo_stub::{serve, Hits, Reply, Request, Stub};

const LEAGUE_KEY: &str = "449.l.12345";
/// The page the throttle lands on: pages 0, 25 and 50 are already in hand.
const THROTTLED_PAGE: u32 = 75;

fn scratch_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "draft-assistant-yahoo-pool-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn client_for(stub: &Stub) -> YahooClient {
    let credentials = YahooCredentials {
        client_id: "dj0yJmk9poolcacheclient".into(),
        client_secret: "top-secret-client-secret".into(),
    };
    let tokens = TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: draft_assistant_lib::yahoo_oauth::now_secs() + 3_600,
    };
    let hosts = YahooHosts {
        api_base: format!("{}/fantasy/v2", stub.base()),
        login_base: stub.base(),
        redirect_uri: "oob".into(),
    };
    YahooClient::with_hosts(credentials, tokens, hosts).with_retry(RetryPolicy::fast())
}

/// A full page of `count` players starting at `start`.
fn page(start: u32, count: u32) -> String {
    let mut members = serde_json::Map::new();
    for index in 0..count {
        let id = start + index;
        members.insert(
            index.to_string(),
            serde_json::json!({"player": [[
                {"player_key": format!("449.p.{id}")},
                {"player_id": id.to_string()},
                {"name": {"full": format!("Player {id}"), "first": "Player", "last": id.to_string()}},
                {"editorial_team_abbr": "Sea"},
                {"display_position": "WR"},
                {"eligible_positions": [{"position": "WR"}]}
            ]]}),
        );
    }
    members.insert("count".into(), serde_json::json!(count));
    serde_json::json!({"fantasy_content": {"league": [
        {"league_key": LEAGUE_KEY}, {"players": members}
    ]}})
    .to_string()
}

/// The `start=` matrix parameter of every request the stub saw.
fn starts(stub: &Stub) -> Vec<String> {
    stub.requests()
        .iter()
        .map(|request| {
            request
                .path()
                .split(';')
                .find(|part| part.starts_with("start="))
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn a_throttled_pool_is_kept_and_the_next_load_resumes_on_the_page_it_stopped_on() {
    let hits = Hits::new();
    let counter = hits.clone();
    let stub = serve(move |request: &Request| {
        let path = request.path().to_string();
        // The throttle clears once: the first load meets it on every attempt
        // and gives up; the second load gets through.
        if path.contains(&format!("start={THROTTLED_PAGE}"))
            && counter.bump("throttled page") <= RetryPolicy::fast().attempts
        {
            return Reply::throttled(0);
        }
        for start in [0u32, 25, 50, THROTTLED_PAGE] {
            if path.contains(&format!("start={start}")) {
                return Reply::ok(page(start, PAGE));
            }
        }
        Reply::ok(page(100, 2))
    });
    let dir = scratch_dir("resume");
    let engine = Engine::new(dir.clone());
    let client = client_for(&stub);

    let (partial, warning) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the pages that did arrive are worth having");
    assert_eq!(
        partial.len() as u32,
        THROTTLED_PAGE,
        "the three pages fetched before the throttle were thrown away"
    );
    let warning = warning.expect("a partial pool is worth saying out loud");
    assert!(warning.contains("part of the player pool"), "{warning}");

    // The partial pool is on disk, marked as partial. Read straight off the
    // file: the cache envelope is `{fetched_at, data}` and this is asserting
    // on what a later launch would find there.
    let text = std::fs::read_to_string(dir.join(cache_name(LEAGUE_KEY, "players")))
        .expect("the partial pool was cached");
    let envelope: serde_json::Value = serde_json::from_str(&text).expect("valid cache JSON");
    let cached: PlayerPool =
        serde_json::from_value(envelope["data"].clone()).expect("a pool in the cache");
    assert!(!cached.complete, "a partial pool was cached as a whole one");
    assert_eq!(cached.next_start, THROTTLED_PAGE);

    let before = stub.count();
    let (whole, warning) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the second load finishes the pool");
    assert_eq!(whole.len() as u32, PAGE * 4 + 2, "the pool came back whole");
    assert_eq!(warning, None);
    let resumed: Vec<String> = starts(&stub).split_off(before);
    assert_eq!(
        resumed,
        vec![format!("start={THROTTLED_PAGE}"), "start=100".to_string()],
        "the second load went back to page zero instead of resuming"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_pool_that_finished_is_served_from_the_cache_without_a_request() {
    let stub = serve(|request: &Request| {
        if request.path().contains("start=0") {
            Reply::ok(page(0, 3))
        } else {
            Reply::status(404, "{}")
        }
    });
    let dir = scratch_dir("complete");
    let engine = Engine::new(dir.clone());
    let client = client_for(&stub);
    let (first, _) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the pool loads");
    assert_eq!(first.len(), 3);
    let (second, warning) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the cache answers");
    assert_eq!(second.len(), 3);
    assert_eq!(warning, None);
    assert_eq!(stub.count(), 1, "a complete, fresh pool was fetched twice");
    std::fs::remove_dir_all(&dir).ok();
}

/// Write a pool straight into the cache, aged `age_secs` seconds. That is how
/// a partial from a previous session looks on the next launch.
fn plant_pool(dir: &std::path::Path, pool: &PlayerPool, age_secs: u64) {
    std::fs::create_dir_all(dir).expect("the cache directory");
    let fetched_at = draft_assistant_lib::yahoo_oauth::now_secs().saturating_sub(age_secs);
    let envelope = serde_json::json!({"fetched_at": fetched_at, "data": pool});
    std::fs::write(
        dir.join(cache_name(LEAGUE_KEY, "players")),
        envelope.to_string(),
    )
    .expect("plant the cache");
}

/// A partial pool holding `rows` players and claiming to have walked as far
/// as `next_start`.
fn partial(rows: u32, next_start: u32) -> PlayerPool {
    let page: serde_json::Value =
        serde_json::from_str(&page(0, rows)).expect("the page is valid JSON");
    PlayerPool {
        players: draft_assistant_lib::yahoo_parse::players(&page).players,
        next_start,
        complete: false,
    }
}

#[tokio::test]
async fn a_partial_pool_from_hours_ago_is_walked_again_from_the_top() {
    // The failure this prevents: the offsets in a partial pool are positions
    // in a list Yahoo re-orders as players are added and dropped. Resuming an
    // old one at offset 75 skipped whatever had moved above that line, read
    // the rest twice, and then marked the result complete, so nothing ever
    // noticed the hole.
    let stub = serve(|request: &Request| {
        let path = request.path().to_string();
        for start in [0u32, 25, 50] {
            if path.contains(&format!("start={start}")) {
                return Reply::ok(page(start, PAGE));
            }
        }
        Reply::ok(page(75, 2))
    });
    let dir = scratch_dir("stale-resume");
    let engine = Engine::new(dir.clone());
    let client = client_for(&stub);
    plant_pool(&dir, &partial(PAGE, 75), 4 * 3_600);

    let (pool, warning) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the pool loads");
    assert_eq!(warning, None, "a clean walk from the top says nothing");
    assert_eq!(
        starts(&stub)[0],
        "start=0",
        "an hours-old partial was resumed instead of restarted"
    );
    assert_eq!(pool.len() as u32, PAGE * 3 + 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_resumed_walk_that_does_not_add_up_is_not_reported_as_a_whole_pool() {
    // A cache claiming to have read three pages while holding two players is
    // not the pool those offsets describe. Resuming it produced a pool short
    // by seventy rows and marked it complete, so every later load served the
    // hole from disk without a request.
    let stub = serve(|request: &Request| {
        if request.path().contains("start=75") {
            Reply::ok(page(75, 2))
        } else {
            Reply::ok(page(0, PAGE))
        }
    });
    let dir = scratch_dir("short-resume");
    let engine = Engine::new(dir.clone());
    let client = client_for(&stub);
    plant_pool(&dir, &partial(2, 75), 60);

    let (pool, warning) = engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("what did arrive is still worth having");
    assert_eq!(
        starts(&stub)[0],
        "start=75",
        "the fresh partial was resumed"
    );
    assert!(pool.len() < 75, "the walk did not actually have a hole");
    let warning = warning.expect("a pool that does not add up is worth saying out loud");
    assert!(warning.contains("players"), "{warning}");
    assert!(warning.contains("from the start"), "{warning}");

    // …and the cache it left behind starts the next load clean rather than
    // carrying the same hole forward.
    let text = std::fs::read_to_string(dir.join(cache_name(LEAGUE_KEY, "players")))
        .expect("the pool was cached");
    let envelope: serde_json::Value = serde_json::from_str(&text).expect("valid cache JSON");
    let cached: PlayerPool =
        serde_json::from_value(envelope["data"].clone()).expect("a pool in the cache");
    assert!(!cached.complete);
    assert_eq!(cached.next_start, 0);
    assert!(cached.players.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_poll_tick_can_name_a_player_and_say_he_was_kept_without_asking_yahoo() {
    // The failure this prevents: a tick built its picks against an empty
    // player map, so every pick made after the load lost its name, its
    // position and its keeper flag three seconds after the board was built.
    let stub = serve(|request: &Request| {
        if request.path().contains("start=0") {
            Reply::ok(page(0, 3))
        } else {
            Reply::status(404, "{}")
        }
    });
    let dir = scratch_dir("pick-context");
    let engine = Engine::new(dir.clone());
    let client = client_for(&stub);
    engine
        .yahoo_pool(&client, LEAGUE_KEY, false)
        .await
        .expect("the pool loads");
    engine
        .save_yahoo_rosters(
            LEAGUE_KEY,
            &[
                YahooPlayer {
                    player_key: "449.p.1".into(),
                    is_keeper: Some(true),
                    ..YahooPlayer::default()
                },
                // On a roster and not in the pool, which is the usual case:
                // the pool is what is still available.
                YahooPlayer {
                    player_key: "449.p.900".into(),
                    full_name: "Kept Elsewhere".into(),
                    display_position: "TE".into(),
                    is_keeper: Some(true),
                    ..YahooPlayer::default()
                },
            ],
        )
        .await;

    let before = stub.count();
    let context = engine.yahoo_pick_context(LEAGUE_KEY).await;
    assert_eq!(
        stub.count(),
        before,
        "a tick went back to Yahoo for the pool"
    );
    let player = context
        .get("449.p.1")
        .expect("the pool the load cached names this player");
    assert_eq!(player.full_name, "Player 1");
    assert_eq!(player.display_position, "WR");
    assert_eq!(player.is_keeper, Some(true), "the keeper flag was lost");
    assert_eq!(
        context.get("449.p.2").expect("a second player").is_keeper,
        None,
        "a player nobody flagged must not read as 'not a keeper'"
    );
    let elsewhere = context
        .get("449.p.900")
        .expect("a kept player who is not in the pool is still describable");
    assert_eq!(elsewhere.full_name, "Kept Elsewhere");
    assert_eq!(elsewhere.is_keeper, Some(true));
    std::fs::remove_dir_all(&dir).ok();
}
