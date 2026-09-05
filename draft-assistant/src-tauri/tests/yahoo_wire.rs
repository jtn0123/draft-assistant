//! The Yahoo client end to end against a stub that serves the fixtures.
//!
//! The stub records every request, so these tests assert on what actually
//! went out — the `Bearer` header, the `format=json` query, the exact matrix
//! parameters of a players page — as well as on what came back. The token
//! side of the wire (exchange, refresh, 401-then-retry) is next door in
//! `tests/yahoo_auth_wire.rs`; the stub they share is `tests/yahoo_stub/`.
//! Nothing here leaves the machine.

mod yahoo_stub;

use draft_assistant_lib::yahoo::{YahooClient, YahooError, YahooHosts, NFL};
use draft_assistant_lib::yahoo_oauth::{TokenSet, YahooCredentials};
use draft_assistant_lib::yahoo_retry::RetryPolicy;
use std::time::Duration;
use yahoo_stub::{serve, Hits, Reply, Request, Stub};

const USER_LEAGUES: &str = include_str!("fixtures/yahoo/user_leagues.json");
const LEAGUE: &str = include_str!("fixtures/yahoo/league_settings.json");
const TEAMS: &str = include_str!("fixtures/yahoo/teams.json");
const PREDRAFT: &str = include_str!("fixtures/yahoo/draft_results_predraft.json");
const PARTIAL: &str = include_str!("fixtures/yahoo/draft_results_partial.json");
const COMPLETE: &str = include_str!("fixtures/yahoo/draft_results_complete.json");
const AUCTION: &str = include_str!("fixtures/yahoo/draft_results_auction.json");
const PLAYERS_0: &str = include_str!("fixtures/yahoo/players_page_0.json");
const PLAYERS_1: &str = include_str!("fixtures/yahoo/players_page_1.json");
const ROSTER: &str = include_str!("fixtures/yahoo/team_roster.json");
const LEAGUE_KEY: &str = "449.l.12345";

const SECRET: &str = "top-secret-client-secret";

fn credentials() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9wireclient".into(),
        client_secret: SECRET.into(),
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

/// The real client waits a second, then two, then four between attempts. A
/// test that asserts on how many attempts there were should not sit through
/// that, so every client here retries as often on millisecond waits.
fn client_for(stub: &Stub, tokens: TokenSet) -> YahooClient {
    YahooClient::with_hosts(credentials(), tokens, hosts(stub)).with_retry(RetryPolicy::fast())
}

fn hosts(stub: &Stub) -> YahooHosts {
    YahooHosts {
        api_base: format!("{}/fantasy/v2", stub.base()),
        login_base: stub.base(),
        redirect_uri: "oob".into(),
    }
}

/// The routes the fixtures cover, for the tests that only want a happy path.
fn fixture_route(request: &Request) -> Reply {
    let path = request.path();
    if path.ends_with("/leagues") {
        Reply::ok(USER_LEAGUES)
    } else if path.ends_with("/settings") {
        Reply::ok(LEAGUE)
    } else if path.ends_with("/teams") {
        Reply::ok(TEAMS)
    } else if path.ends_with("/draftresults") {
        Reply::ok(PARTIAL)
    } else if path.ends_with("/roster") {
        Reply::ok(ROSTER)
    } else if path.contains("/players;start=0") {
        Reply::ok(PLAYERS_0)
    } else if path.contains("/players") {
        Reply::ok(PLAYERS_1)
    } else {
        Reply::status(404, r#"{"error":{"description":"no such resource"}}"#)
    }
}

#[tokio::test]
async fn every_request_carries_the_bearer_token_and_asks_for_json() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let leagues = client.user_leagues(NFL).await.expect("the leagues load");
    assert_eq!(leagues.len(), 2);

    let request = stub.requests().pop().expect("one request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.header("authorization"), Some("Bearer access-1"));
    assert_eq!(request.query(), "format=json");
    assert_eq!(
        request.path(),
        "/fantasy/v2/users;use_login=1/games;game_keys=nfl/leagues"
    );
}

#[tokio::test]
async fn the_league_call_asks_for_settings_and_comes_back_whole() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let league = client.league(LEAGUE_KEY).await.expect("the league loads");
    assert_eq!(league.league_key, LEAGUE_KEY);
    assert_eq!(league.num_teams, 12);
    assert_eq!(league.roster_positions.len(), 10);
    assert_eq!(league.stat_modifiers.len(), 30);
    assert_eq!(
        stub.requests()[0].path(),
        "/fantasy/v2/league/449.l.12345/settings"
    );
}

#[tokio::test]
async fn a_yahoo_league_maps_onto_the_apps_own_shape() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let league = client.league(LEAGUE_KEY).await.expect("the league loads");
    let mapped = draft_assistant_lib::yahoo_map::league(&league);
    assert_eq!(mapped.league_id, LEAGUE_KEY);
    assert_eq!(mapped.status, "pre_draft");
    assert_eq!(mapped.total_rosters, 12);
    assert_eq!(
        mapped
            .roster_positions
            .iter()
            .filter(|p| *p == "BN")
            .count(),
        6
    );
    assert!(mapped.roster_positions.contains(&"FLEX".to_string()));
    assert!(mapped.roster_positions.contains(&"SUPER_FLEX".to_string()));
    assert_eq!(mapped.scoring_settings.get("rec"), Some(&0.5));
    assert_eq!(mapped.scoring_settings.get("pass_yd"), Some(&0.04));
}

#[tokio::test]
async fn the_teams_load_with_their_managers_and_draft_slots() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let teams = client.league_teams(LEAGUE_KEY).await.expect("teams load");
    assert_eq!(teams.len(), 3);
    assert_eq!(teams[0].draft_position, Some(1));
    assert!(teams[0].managers[0].is_current_login);
    assert_eq!(
        stub.requests()[0].path(),
        "/fantasy/v2/league/449.l.12345/teams"
    );
}

#[tokio::test]
async fn a_draft_reads_empty_then_partial_then_whole() {
    let hits = Hits::new();
    let counter = hits.clone();
    let stub = serve(move |request: &Request| {
        if !request.path().ends_with("/draftresults") {
            return Reply::status(404, "{}");
        }
        match counter.bump("draft") {
            1 => Reply::ok(PREDRAFT),
            2 => Reply::ok(PARTIAL),
            _ => Reply::ok(COMPLETE),
        }
    });
    let client = client_for(&stub, live_tokens());

    assert!(client
        .draft_results(LEAGUE_KEY)
        .await
        .expect("before the draft")
        .is_empty());

    let during = client.draft_results(LEAGUE_KEY).await.expect("during");
    assert_eq!(during.len(), 4);
    assert_eq!(during[0].player_key, "449.p.30977");

    let after = client.draft_results(LEAGUE_KEY).await.expect("after");
    assert_eq!(after.len(), 6);
    assert_eq!(stub.count(), 3);
}

#[tokio::test]
async fn the_picks_a_draft_has_made_become_the_apps_picks() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let results = client.draft_results(LEAGUE_KEY).await.expect("picks");
    let teams = client.league_teams(LEAGUE_KEY).await.expect("teams");
    let picks = draft_assistant_lib::yahoo_map::picks(&results, &teams, &Default::default());
    // The fourth row has no player yet, so it is not a pick.
    assert_eq!(picks.len(), 3);
    assert_eq!(picks[0].draft_slot, 1);
    assert_eq!(picks[0].player_id, "yahoo:30977");
    assert_eq!(picks[2].draft_slot, 0, "the late joiner has no slot yet");
}

#[tokio::test]
async fn an_auction_draft_keeps_its_costs_across_the_wire() {
    let stub = serve(|_: &Request| Reply::ok(AUCTION));
    let client = client_for(&stub, live_tokens());
    let results = client.draft_results("449.l.67890").await.expect("picks");
    let costs = draft_assistant_lib::yahoo_map::auction_costs(&results);
    assert_eq!(costs.get("yahoo:30977"), Some(&55.0));
    assert_eq!(costs.len(), 3);
}

#[tokio::test]
async fn one_page_of_players_is_asked_for_exactly_as_yahoo_spells_it() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let page = client
        .players(LEAGUE_KEY, 0, 25, Some("WR"))
        .await
        .expect("a page");
    assert_eq!(page.players.len(), 2);
    let request = &stub.requests()[0];
    assert_eq!(
        request.path(),
        "/fantasy/v2/league/449.l.12345/players;start=0;count=25;position=WR"
    );
    assert_eq!(request.query(), "format=json");
}

#[tokio::test]
async fn a_team_roster_loads_through_the_team_resource() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let roster = client
        .team_roster("449.l.12345.t.1")
        .await
        .expect("the roster loads");
    assert_eq!(roster.len(), 2);
    assert_eq!(
        stub.requests()[0].path(),
        "/fantasy/v2/team/449.l.12345.t.1/roster"
    );
}

#[tokio::test]
async fn a_5xx_is_tried_again_and_the_third_answer_is_kept() {
    let hits = Hits::new();
    let counter = hits.clone();
    let stub = serve(move |_: &Request| match counter.bump("api") {
        1 | 2 => Reply::status(503, r#"{"error":"busy"}"#),
        _ => Reply::ok(TEAMS),
    });
    let client = client_for(&stub, live_tokens());
    let teams = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect("the third try");
    assert_eq!(teams.len(), 3);
    assert_eq!(stub.count(), 3);
}

#[tokio::test]
async fn a_5xx_that_never_clears_fails_after_the_policys_attempts() {
    let stub = serve(|_: &Request| Reply::status(500, r#"{"error":"down"}"#));
    let client = client_for(&stub, live_tokens());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("Yahoo is down");
    assert!(
        matches!(error, YahooError::Http { status: 500, .. }),
        "{error:?}"
    );
    assert_eq!(stub.count(), 3);
}

#[tokio::test]
async fn a_404_is_not_worth_repeating() {
    let stub = serve(|_: &Request| Reply::status(404, r#"{"error":"no such league"}"#));
    let client = client_for(&stub, live_tokens());
    let error = client
        .league_teams("449.l.99999")
        .await
        .expect_err("no such league");
    assert!(
        matches!(error, YahooError::Http { status: 404, .. }),
        "{error:?}"
    );
    assert_eq!(stub.count(), 1);
}

#[tokio::test]
async fn a_server_that_accepts_and_says_nothing_times_out() {
    let stub = serve(|_: &Request| Reply::Hang(Duration::from_secs(30)));
    let client = YahooClient::with_hosts_timeout(
        credentials(),
        live_tokens(),
        hosts(&stub),
        Duration::from_millis(300),
    )
    .with_retry(RetryPolicy::fast());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("nothing ever arrives");
    assert!(matches!(error, YahooError::Transport { .. }), "{error:?}");
    // A timeout is a blip, so it is retried like one.
    assert_eq!(stub.count(), 3);
}

#[tokio::test]
async fn nothing_listening_is_a_transport_failure_not_a_panic() {
    let client = YahooClient::with_hosts(
        credentials(),
        live_tokens(),
        YahooHosts {
            api_base: "http://127.0.0.1:1/fantasy/v2".into(),
            login_base: "http://127.0.0.1:1".into(),
            redirect_uri: "oob".into(),
        },
    )
    .with_retry(RetryPolicy::fast());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("port 1 answers nobody");
    assert!(matches!(error, YahooError::Transport { .. }), "{error:?}");
}

#[tokio::test]
async fn a_body_that_is_not_json_is_reported_as_such_and_not_retried() {
    let stub = serve(|_: &Request| Reply::ok("<html>maintenance</html>"));
    let client = client_for(&stub, live_tokens());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("that was not JSON");
    assert!(matches!(error, YahooError::Decode { .. }), "{error:?}");
    assert_eq!(stub.count(), 1);
}

#[tokio::test]
async fn a_league_key_that_could_escape_the_path_never_reaches_the_network() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let error = client
        .league_teams("449.l.1/../../users;use_login=1")
        .await
        .expect_err("that is not a league key");
    assert!(matches!(error, YahooError::Invalid(_)), "{error:?}");
    assert_eq!(stub.count(), 0);
}

#[tokio::test]
async fn a_league_the_reply_does_not_contain_is_an_error_not_an_empty_league() {
    let stub = serve(|_: &Request| Reply::ok(r#"{"fantasy_content": {}}"#));
    let client = client_for(&stub, live_tokens());
    let error = client
        .league(LEAGUE_KEY)
        .await
        .expect_err("nothing came back");
    assert!(matches!(error, YahooError::Invalid(_)), "{error:?}");
}

#[tokio::test]
async fn the_players_a_page_hands_back_map_onto_the_apps_rows() {
    let stub = serve(fixture_route);
    let client = client_for(&stub, live_tokens());
    let page = client
        .players(LEAGUE_KEY, 0, 25, None)
        .await
        .expect("a page");
    let mapped = draft_assistant_lib::yahoo_map::players(&page.players);
    assert_eq!(mapped[0].id, "yahoo:30977");
    assert_eq!(mapped[0].meta.team.as_deref(), Some("CIN"));
    assert_eq!(mapped[0].bye_week, Some(10));
    assert_eq!(mapped[1].meta.injury_status.as_deref(), Some("Q"));
}
