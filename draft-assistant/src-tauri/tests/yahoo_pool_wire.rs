//! The player-pool walk against a stub: paging, dropped rows, and throttles.
//!
//! Split out of `tests/yahoo_wire.rs` because the pool is the one Yahoo read
//! that is many requests rather than one, and because its failures are about
//! the sequence of requests rather than about any single one. The stub these
//! share is `tests/yahoo_stub/`, and nothing here leaves the machine.

mod yahoo_stub;

use draft_assistant_lib::yahoo::{YahooClient, YahooHosts, PAGE};
use draft_assistant_lib::yahoo_oauth::{TokenSet, YahooCredentials};
use draft_assistant_lib::yahoo_retry::RetryPolicy;
use std::time::Duration;
use yahoo_stub::{serve, Hits, Reply, Request, Stub};

const LEAGUE_KEY: &str = "449.l.12345";

fn credentials() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9poolclient".into(),
        client_secret: "top-secret-client-secret".into(),
    }
}

/// A token pair that is good for another hour.
fn live_tokens() -> TokenSet {
    TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: draft_assistant_lib::yahoo_oauth::now_secs() + 3_600,
    }
}

fn hosts(stub: &Stub) -> YahooHosts {
    YahooHosts {
        api_base: format!("{}/fantasy/v2", stub.base()),
        login_base: stub.base(),
        redirect_uri: "oob".into(),
    }
}

/// The real client waits a second, then two, then four between attempts. A
/// test that only counts requests should not sit through that.
fn client_for(stub: &Stub) -> YahooClient {
    YahooClient::with_hosts(credentials(), live_tokens(), hosts(stub))
        .with_retry(RetryPolicy::fast())
}

/// A full page of `count` players, so the pager has a reason to ask again.
fn full_page(start: u32, count: u32) -> String {
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

#[tokio::test]
async fn paging_walks_until_a_page_comes_back_short() {
    let stub = serve(|request: &Request| {
        if request.path().contains(&format!("start={}", PAGE)) {
            Reply::ok(full_page(PAGE, 3))
        } else if request.path().contains("start=0") {
            Reply::ok(full_page(0, PAGE))
        } else {
            Reply::status(404, "{}")
        }
    });
    let client = client_for(&stub);
    let players = client
        .all_players(LEAGUE_KEY, None, 500)
        .await
        .expect("the pool loads");
    assert_eq!(players.len() as u32, PAGE + 3);
    assert_eq!(stub.count(), 2, "a short page ends the walk");
    assert_eq!(players[0].player_key, "449.p.0");
    assert_eq!(players[PAGE as usize].player_key, "449.p.25");
}

#[tokio::test]
async fn paging_stops_at_the_limit_even_if_yahoo_keeps_answering() {
    let stub = serve(|_: &Request| Reply::ok(full_page(0, PAGE)));
    let client = client_for(&stub);
    let players = client
        .all_players(LEAGUE_KEY, None, PAGE * 2)
        .await
        .expect("the pool loads");
    assert_eq!(players.len() as u32, PAGE * 2);
    assert_eq!(stub.count(), 2);
}

/// A page of `count` rows where one of them is a row this app cannot read:
/// no `player_key`, which is how Yahoo sends the odd placeholder.
fn page_with_a_dropped_row(start: u32, count: u32) -> String {
    let mut page: serde_json::Value =
        serde_json::from_str(&full_page(start, count)).expect("a page to spoil");
    let players = &mut page["fantasy_content"]["league"][1]["players"];
    players["0"] = serde_json::json!({"player": [[{"player_id": "ghost"}]]});
    page.to_string()
}

#[tokio::test]
async fn one_row_the_parser_drops_does_not_end_the_pool_early() {
    // The failure this prevents: the walk asked whether *the rows it kept*
    // filled the page, so a single unreadable row looked like the end of the
    // pool and the board lost every player after it.
    let stub = serve(|request: &Request| {
        if request.path().contains(&format!("start={PAGE}")) {
            Reply::ok(full_page(PAGE, 3))
        } else if request.path().contains("start=0") {
            Reply::ok(page_with_a_dropped_row(0, PAGE))
        } else {
            Reply::status(404, "{}")
        }
    });
    let client = client_for(&stub);
    let players = client
        .all_players(LEAGUE_KEY, None, 500)
        .await
        .expect("the pool loads");
    assert_eq!(stub.count(), 2, "the walk stopped on the dropped row");
    assert_eq!(players.len() as u32, PAGE - 1 + 3);
}

#[tokio::test]
async fn a_throttled_page_waits_as_long_as_yahoo_asked_and_resumes_on_that_page() {
    // The failure this prevents: a 999 partway through the pool waited 250ms,
    // gave up, and threw away every page already fetched.
    let hits = Hits::new();
    let counter = hits.clone();
    let stub = serve(move |request: &Request| {
        let path = request.path().to_string();
        if path.contains("start=50") && counter.bump("third page") == 1 {
            return Reply::throttled(1);
        }
        for start in [0u32, 25, 50] {
            if path.contains(&format!("start={start}")) {
                return Reply::ok(full_page(start, PAGE));
            }
        }
        Reply::ok(full_page(75, 2))
    });
    let client = YahooClient::with_hosts(credentials(), live_tokens(), hosts(&stub)).with_retry(
        RetryPolicy {
            attempts: 3,
            base: Duration::from_millis(5),
            cap: Duration::from_secs(5),
            jitter: false,
        },
    );
    let started = std::time::Instant::now();
    let players = client
        .all_players(LEAGUE_KEY, None, 500)
        .await
        .expect("the pool loads through the throttle");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "Retry-After: 1 was not waited out: {:?}",
        started.elapsed()
    );
    assert_eq!(
        players.len() as u32,
        PAGE * 3 + 2,
        "the pool came back whole"
    );
    let starts: Vec<String> = stub
        .requests()
        .iter()
        .map(|request| {
            request
                .path()
                .split(';')
                .find(|part| part.starts_with("start="))
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        starts,
        vec!["start=0", "start=25", "start=50", "start=50", "start=75"],
        "the retry went back to page zero instead of resuming"
    );
}
