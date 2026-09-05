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
