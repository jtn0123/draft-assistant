//! The command surface behind the desktop app, exercised without Tauri:
//! loading, identity, refreshes, manual picks and their rollback, export,
//! Ask Claude through a stub CLI, and the live poll loop's state machine.

mod support;

use draft_assistant_lib::app::{AppCore, PollEvent};
use draft_assistant_lib::chat::{ChatOptions, ChatTurn};
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::sleeper::SleeperClient;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{pick, Fixture, Reply, StubSleeper, LEAGUE_ID, MY_USER, MY_USERNAME};

struct Rig {
    stub: StubSleeper,
    fixture: Fixture,
    core: Arc<AppCore>,
}

fn make_rig(label: &str) -> Rig {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = Engine {
        client: SleeperClient::with_base_url(&stub.base),
        data_dir: support::scratch_dir(label),
    };
    Rig {
        stub,
        fixture,
        core: Arc::new(AppCore::new(engine)),
    }
}

async fn loaded_rig(label: &str) -> Rig {
    let rig = make_rig(label);
    rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    rig.core.set_my_username(MY_USERNAME).await.unwrap();
    rig
}

#[tokio::test]
async fn nothing_works_before_a_league_is_loaded() {
    let rig = make_rig("empty");
    assert_eq!(rig.core.get_state().await.unwrap_err(), "no league loaded");
    assert_eq!(
        rig.core.refresh_picks().await.unwrap_err(),
        "no league loaded"
    );
    assert_eq!(
        rig.core.refresh_data().await.unwrap_err(),
        "no active league"
    );
    assert_eq!(
        rig.core
            .record_manual_pick("qb-1".into())
            .await
            .unwrap_err(),
        "no league loaded"
    );
    assert_eq!(
        rig.core.undo_manual_pick().await.unwrap_err(),
        "no league loaded"
    );
    assert_eq!(
        rig.core.export_state().await.unwrap_err(),
        "no league loaded"
    );
    let err = rig
        .core
        .ask("Who?", &[], &ChatOptions::default(), &mut |_| {})
        .await
        .unwrap_err();
    assert_eq!(err, "no league loaded");
    assert!(rig.core.get_config().await.active_league_id.is_none());
}

#[tokio::test]
async fn adding_a_league_makes_it_active_remembers_it_and_serves_its_state() {
    let rig = make_rig("add");
    let view = rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(view.league.name, "Mixed lineup fixture");
    assert_eq!(view.draft.current_pick, 1);
    assert!(view.draft.my_slot.is_none(), "no username yet");

    let config = rig.core.get_config().await;
    assert_eq!(config.active_league_id.as_deref(), Some(LEAGUE_ID));
    assert_eq!(config.leagues.len(), 1);
    assert_eq!(config.leagues[0].name, "Mixed lineup fixture");
    // Persisted: a fresh core over the same directory sees it.
    let again = AppCore::new(Engine {
        client: SleeperClient::with_base_url(&rig.stub.base),
        data_dir: rig.core.engine.data_dir.clone(),
    });
    assert_eq!(
        again.get_config().await.active_league_id.as_deref(),
        Some(LEAGUE_ID)
    );
    // Adding it twice does not duplicate the entry.
    rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(rig.core.get_config().await.leagues.len(), 1);
    assert_eq!(
        rig.core.get_state().await.unwrap().league.league_id,
        LEAGUE_ID
    );
}

#[tokio::test]
async fn the_username_resolves_my_slot_and_an_unknown_one_is_refused() {
    let rig = make_rig("username");
    rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(
        rig.core.set_my_username(MY_USERNAME).await.unwrap(),
        MY_USER
    );
    let view = rig.core.get_state().await.unwrap();
    assert_eq!(view.draft.my_slot, Some(1));
    assert!(view.my_roster.is_some());
    assert_eq!(
        rig.core.set_my_username("nobody").await.unwrap_err(),
        "Sleeper user 'nobody' not found"
    );
    rig.stub.set("/v1/user/down", Reply::Status(500));
    assert!(rig
        .core
        .set_my_username("down")
        .await
        .unwrap_err()
        .contains("HTTP 500"));
}

#[tokio::test]
async fn refresh_picks_applies_new_picks_and_the_draft_status() {
    let rig = loaded_rig("refresh-picks").await;
    rig.fixture
        .set_picks(&rig.stub, &[pick(1, 1, "qb-1"), pick(2, 2, "rb-1")]);
    rig.fixture
        .set_status(&rig.stub, "drafting", Some(1_700_000_000_000));
    let view = rig.core.refresh_picks().await.unwrap();
    assert_eq!(view.draft.status, "drafting");
    assert_eq!(view.draft.total_picks_made, 2);
    assert_eq!(view.draft.current_pick, 3);
    assert!(view.draft.pick_deadline.is_some());
    assert_eq!(view.my_roster.as_ref().unwrap().players.len(), 1);
    assert!(view.available.iter().all(|p| p.player.player_id != "qb-1"));
    assert_eq!(view.data_health.poll_consecutive_failures, 0);

    // A failed picks fetch is an error here — the user asked.
    rig.stub
        .set("/v1/draft/fixture-draft/picks", Reply::Status(500));
    assert!(rig
        .core
        .refresh_picks()
        .await
        .unwrap_err()
        .contains("HTTP 500"));
}

#[tokio::test]
async fn refresh_data_rebuilds_from_the_network_and_the_board_survives() {
    let rig = loaded_rig("refresh-data").await;
    rig.stub.reset_hits();
    let view = rig.core.refresh_data().await.unwrap();
    assert_eq!(
        rig.stub.hits("/v1/players/nfl"),
        1,
        "force bypasses the cache"
    );
    assert_eq!(view.available.len(), 6);
    assert_eq!(
        view.draft.my_slot,
        Some(1),
        "identity is kept across a rebuild"
    );
}

#[tokio::test]
async fn a_manual_pick_is_applied_persisted_and_undone_and_a_failed_save_rolls_back() {
    let rig = loaded_rig("manual").await;
    let view = rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    assert!(view.draft.manual_picks_active);
    assert_eq!(view.draft.current_pick, 2);
    assert!(view.available.iter().all(|p| p.player.player_id != "rb-1"));
    assert_eq!(view.my_roster.unwrap().players[0].player_id, "rb-1");
    assert!(
        rig.core
            .engine
            .data_dir
            .join("manual_picks_fixture-draft.json")
            .is_file(),
        "the pick is on disk before it is on screen"
    );
    let err = rig
        .core
        .record_manual_pick("no-such-player".into())
        .await
        .unwrap_err();
    assert!(!err.is_empty());

    let view = rig.core.undo_manual_pick().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.available.len(), 6);
    assert!(
        rig.core.undo_manual_pick().await.is_err(),
        "nothing left to undo"
    );

    // The data directory vanishes: the pick must not appear on the board.
    std::fs::remove_dir_all(&rig.core.engine.data_dir).unwrap();
    let err = rig
        .core
        .record_manual_pick("wr-1".into())
        .await
        .unwrap_err();
    assert!(err.contains("write"), "{err}");
    let view = rig.core.get_state().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.available.len(), 6);
}

#[tokio::test]
async fn live_picks_supersede_manual_ones() {
    let rig = loaded_rig("supersede").await;
    rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    // The API now reports pick 1 as someone else; the manual pick is dropped.
    rig.fixture.set_picks(&rig.stub, &[pick(1, 1, "qb-1")]);
    let view = rig.core.refresh_picks().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.draft.total_picks_made, 1);
    assert!(rig.core.undo_manual_pick().await.is_err());
}

#[tokio::test]
async fn export_writes_the_same_view_the_ui_renders() {
    let rig = loaded_rig("export").await;
    let path = rig.core.export_state().await.unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], "1.3");
    assert_eq!(value["league"]["league_id"], LEAGUE_ID);
    assert_eq!(value["available"].as_array().unwrap().len(), 6);
    assert!(path.ends_with("draft-state.json"));
}

#[tokio::test]
async fn ask_streams_the_answer_and_stamps_the_pick_it_saw() {
    let rig = loaded_rig("ask").await;
    let claude = support::stub_claude(&rig.core.engine.data_dir, "Take Fixture QB.");
    std::env::set_var("DRAFT_ASSISTANT_CLAUDE_BIN", &claude);
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let sink = chunks.clone();
    let reply = rig
        .core
        .ask("Who?", &[], &ChatOptions::default(), &mut |t| {
            sink.lock().unwrap().push(t.to_string())
        })
        .await
        .unwrap();
    assert_eq!(reply.answer, "Take Fixture QB.");
    assert_eq!(chunks.lock().unwrap().concat(), "Take Fixture QB.");
    assert_eq!(chunks.lock().unwrap().len(), 2, "two streamed pieces");
    let as_of = reply.as_of.unwrap();
    assert_eq!(as_of.pick, 1);
    assert!(as_of.seq > 0);
    assert_eq!(reply.usage.cost_usd, Some(0.05));
    assert_eq!(reply.usage.context_tokens, 900);

    let history = vec![
        ChatTurn {
            role: "you".into(),
            text: "Who?".into(),
        },
        ChatTurn {
            role: "claude".into(),
            text: reply.answer,
        },
    ];
    let summary = rig
        .core
        .compact(&history, &ChatOptions::default())
        .await
        .unwrap();
    assert_eq!(summary.answer, "Take Fixture QB.");
    assert!(summary.as_of.is_none(), "a compaction is not about a pick");
    std::env::remove_var("DRAFT_ASSISTANT_CLAUDE_BIN");
}

/// Wait until `events` satisfies `done`, or fail after five seconds.
async fn wait_for(events: &Arc<Mutex<Vec<PollEvent>>>, done: impl Fn(&[PollEvent]) -> bool) {
    let started = std::time::Instant::now();
    loop {
        if done(&events.lock().unwrap()) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out; events: {:?}",
            events.lock().unwrap().len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn views(events: &[PollEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, PollEvent::View(_)))
        .count()
}

fn last_health(events: &[PollEvent]) -> Option<draft_assistant_lib::view::PollHealth> {
    events.iter().rev().find_map(|e| match e {
        PollEvent::Health(h) => Some(h.clone()),
        _ => None,
    })
}

#[tokio::test]
async fn the_poll_loop_emits_a_view_only_when_the_feed_changes_and_reports_failures() {
    let rig = loaded_rig("poll").await;
    let events: Arc<Mutex<Vec<PollEvent>>> = Arc::default();
    let generation = rig.core.begin_polling();
    let (core, sink) = (rig.core.clone(), events.clone());
    let task = tokio::spawn(async move {
        let emit = move |event: PollEvent| sink.lock().unwrap().push(event);
        core.poll_loop(Duration::from_millis(25), generation, &emit)
            .await;
    });

    // First poll: a health report and one view (nothing to compare against yet).
    wait_for(&events, |e| views(e) == 1).await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(views(&events.lock().unwrap()), 1, "no change, no re-emit");
    assert!(last_health(&events.lock().unwrap())
        .unwrap()
        .last_success_at
        .is_some());

    // A pick lands: exactly one more view, carrying it.
    rig.fixture.set_picks(&rig.stub, &[pick(1, 1, "qb-1")]);
    wait_for(&events, |e| views(e) == 2).await;
    let latest = events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|e| match e {
            PollEvent::View(v) => Some((**v).clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(latest.draft.total_picks_made, 1);
    assert_eq!(latest.recent_picks[0].player_id, "qb-1");

    // The status flips without a new pick: still a change.
    rig.fixture
        .set_status(&rig.stub, "drafting", Some(1_700_000_000_000));
    wait_for(&events, |e| views(e) == 3).await;

    // The feed breaks: health counts failures with the reason; no new view.
    rig.stub
        .set("/v1/draft/fixture-draft/picks", Reply::Status(500));
    wait_for(&events, |e| {
        last_health(e)
            .map(|h| h.consecutive_failures >= 2)
            .unwrap_or(false)
    })
    .await;
    let health = last_health(&events.lock().unwrap()).unwrap();
    assert!(health.last_error.as_deref().unwrap().contains("HTTP 500"));
    assert_eq!(views(&events.lock().unwrap()), 3);

    // It recovers: failures reset.
    rig.fixture.set_picks(&rig.stub, &[pick(1, 1, "qb-1")]);
    wait_for(&events, |e| {
        last_health(e)
            .map(|h| h.consecutive_failures == 0)
            .unwrap_or(false)
    })
    .await;

    // Stopping ends the loop.
    rig.core.stop_polling();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the loop exits once polling is off")
        .unwrap();
}

#[tokio::test]
async fn starting_polling_again_supersedes_the_running_loop() {
    let rig = loaded_rig("poll-generation").await;
    let first = rig.core.begin_polling();
    let (core, events) = (rig.core.clone(), Arc::<Mutex<Vec<PollEvent>>>::default());
    let sink = events.clone();
    let task = tokio::spawn(async move {
        let emit = move |event: PollEvent| sink.lock().unwrap().push(event);
        core.poll_loop(Duration::from_millis(25), first, &emit)
            .await;
    });
    wait_for(&events, |e| !e.is_empty()).await;
    let second = rig.core.begin_polling();
    assert_eq!(second, first + 1);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("the superseded loop exits")
        .unwrap();
    // The no-league case: a loop with nothing loaded emits nothing and stops.
    let bare = make_rig("poll-bare");
    let generation = bare.core.begin_polling();
    let mut fingerprint = None;
    assert!(bare.core.poll_once(&mut fingerprint).await.is_none());
    bare.core.stop_polling();
    bare.core
        .poll_loop(Duration::from_millis(1), generation, &|_| {})
        .await;
}

#[tokio::test]
async fn a_keeper_is_remembered_once_the_draft_passes_it() {
    // Pick 5 is in the book before pick 1 is made: a keeper, flag or no flag.
    let rig = make_rig("keeper-memory");
    rig.fixture.set_picks(&rig.stub, &[pick(5, 1, "qb-1")]);
    rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    rig.core.set_my_username(MY_USERNAME).await.unwrap();
    let view = rig.core.get_state().await.unwrap();
    assert!(view.my_roster.unwrap().players[0].is_keeper);
    assert!(view.recent_picks.is_empty());

    // The draft catches up and passes it; the flag never arrives.
    rig.fixture.set_picks(
        &rig.stub,
        &[
            pick(1, 1, "rb-1"),
            pick(2, 2, "wr-1"),
            pick(3, 2, "te-1"),
            pick(4, 1, "k-1"),
            pick(5, 1, "qb-1"),
            pick(6, 2, "def-1"),
        ],
    );
    let view = rig.core.refresh_picks().await.unwrap();
    assert_eq!(view.draft.current_pick, 7);
    let mine = view.my_roster.unwrap();
    let kept: Vec<(u32, bool)> = mine
        .players
        .iter()
        .map(|p| (p.pick_no, p.is_keeper))
        .collect();
    assert_eq!(kept, vec![(1, false), (4, false), (5, true)]);
    let recent: Vec<u32> = view.recent_picks.iter().map(|p| p.pick_no).collect();
    assert_eq!(
        recent,
        vec![6, 4, 3, 2, 1],
        "the keeper is not a recent pick"
    );
}
