//! Test doubles shared by the integration tests: a stub Sleeper API that
//! serves the sanitised fixture league, a stub `claude` CLI that streams a
//! canned answer, and scratch directories.
//!
//! The stub server is a plain-thread HTTP/1.1 responder: one route table,
//! swappable per test (a 500, a hang, a new pick list), with a hit counter
//! so cache behaviour can be asserted from what was actually requested.

#![allow(dead_code)]

use draft_assistant_lib::sleeper::{Draft, League, Pick, ProjectionRow};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum Reply {
    Json(String),
    Status(u16),
    /// Accept the connection and never answer.
    Hang,
}

pub struct StubSleeper {
    pub base: String,
    routes: Arc<Mutex<HashMap<String, Reply>>>,
    hits: Arc<Mutex<Vec<String>>>,
    held: Arc<Mutex<Vec<TcpStream>>>,
}

impl StubSleeper {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Arc<Mutex<HashMap<String, Reply>>> = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let held: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let (r, h, k) = (routes.clone(), hits.clone(), held.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (r, h, k) = (r.clone(), h.clone(), k.clone());
                std::thread::spawn(move || serve(stream, &r, &h, &k));
            }
        });
        Self {
            base: format!("http://{addr}"),
            routes,
            hits,
            held,
        }
    }

    pub fn set(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_string(), reply);
    }

    pub fn json<T: serde::Serialize>(&self, path: &str, value: &T) {
        self.set(path, Reply::Json(serde_json::to_string(value).unwrap()));
    }

    pub fn hits(&self, path: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }

    pub fn reset_hits(&self) {
        self.hits.lock().unwrap().clear();
    }
}

fn serve(
    mut stream: TcpStream,
    routes: &Mutex<HashMap<String, Reply>>,
    hits: &Mutex<Vec<String>>,
    held: &Mutex<Vec<TcpStream>>,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // Drain the headers so the client sees a well-formed exchange.
    let mut line = String::new();
    while reader.read_line(&mut line).map(|n| n > 0).unwrap_or(false) && line.trim() != "" {
        line.clear();
    }
    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/").to_string();
    hits.lock().unwrap().push(path.clone());
    let reply = routes
        .lock()
        .unwrap()
        .get(&path)
        .cloned()
        .unwrap_or(Reply::Status(404));
    let (status, body) = match reply {
        Reply::Json(body) => (200, body),
        Reply::Status(code) => (code, format!("{{\"error\":\"stub {code}\"}}")),
        Reply::Hang => {
            held.lock().unwrap().push(stream);
            return;
        }
    };
    let reason = if status == 200 { "OK" } else { "Stub" };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

// ---------- the fixture league ----------

#[derive(Deserialize)]
struct BoardFixture {
    league: League,
    draft: Draft,
    season_rows: Vec<ProjectionRow>,
}

pub const LEAGUE_ID: &str = "fixture-league";
pub const DRAFT_ID: &str = "fixture-draft";
pub const MOCK_DRAFT_ID: &str = "mock-draft";
pub const MY_USER: &str = "fixture-user";
pub const MY_USERNAME: &str = "fixture";
pub const RIVAL_USER: &str = "other-user";

/// The sanitised Sleeper-shaped league in `tests/fixtures/board_input.json`:
/// two teams, eight rounds, six players. Small enough that every request in
/// a load is cheap and every number in a view can be reasoned about.
pub struct Fixture {
    pub league: League,
    pub draft: Draft,
    pub rows: Vec<ProjectionRow>,
}

impl Fixture {
    pub fn load() -> Self {
        let fixture: BoardFixture =
            serde_json::from_str(include_str!("../fixtures/board_input.json")).unwrap();
        let mut draft = fixture.draft;
        draft.draft_order = Some(HashMap::from([
            (MY_USER.to_string(), 1),
            (RIVAL_USER.to_string(), 2),
        ]));
        Self {
            league: fixture.league,
            draft,
            rows: fixture.season_rows,
        }
    }

    pub fn player_ids(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.player_id.clone()).collect()
    }

    /// Register every route a load touches, with no picks made yet.
    pub fn install(&self, stub: &StubSleeper) {
        stub.json(&format!("/v1/league/{LEAGUE_ID}"), &self.league);
        stub.json(
            &format!("/v1/league/{LEAGUE_ID}/users"),
            &serde_json::json!([
                {"user_id": MY_USER, "display_name": "Fixture Manager"},
                {"user_id": RIVAL_USER, "display_name": "Rival"}
            ]),
        );
        stub.json(&format!("/v1/draft/{DRAFT_ID}"), &self.draft);
        stub.json(&format!("/v1/draft/{DRAFT_ID}/picks"), &Vec::<Pick>::new());
        stub.json(
            &format!("/v1/user/{MY_USERNAME}"),
            &serde_json::json!({"user_id": MY_USER, "username": MY_USERNAME}),
        );
        stub.set("/v1/user/nobody", Reply::Json("null".into()));
        let players: HashMap<String, _> = self
            .rows
            .iter()
            .filter_map(|r| r.player.clone().map(|p| (r.player_id.clone(), p)))
            .collect();
        stub.json("/v1/players/nfl", &players);
        stub.json("/projections/nfl/2026", &self.rows);
        for week in 1..=18 {
            stub.json(
                &format!("/projections/nfl/2026/{week}"),
                &Vec::<ProjectionRow>::new(),
            );
        }
        // A leagueless mock draft that carries its roster shape itself.
        let mut mock = self.draft.clone();
        mock.draft_id = MOCK_DRAFT_ID.into();
        mock.settings.slots_qb = Some(1);
        mock.settings.slots_rb = Some(1);
        mock.settings.slots_wr = Some(1);
        mock.settings.slots_te = Some(1);
        mock.settings.slots_flex = Some(1);
        mock.settings.slots_k = Some(1);
        mock.settings.slots_def = Some(1);
        mock.metadata = Some(draft_assistant_lib::sleeper::DraftMetadata {
            name: Some("Mock".into()),
            scoring_type: Some("half_ppr".into()),
        });
        mock.creators = Some(vec![MY_USER.into()]);
        stub.json(&format!("/v1/draft/{MOCK_DRAFT_ID}"), &mock);
        stub.json(
            &format!("/v1/draft/{MOCK_DRAFT_ID}/picks"),
            &Vec::<Pick>::new(),
        );
    }

    /// Publish a pick list, as the API would after picks are made.
    pub fn set_picks(&self, stub: &StubSleeper, picks: &[Pick]) {
        stub.json(&format!("/v1/draft/{DRAFT_ID}/picks"), &picks);
    }

    /// Publish a draft status ("drafting", "complete"), optionally with a clock.
    pub fn set_status(&self, stub: &StubSleeper, status: &str, last_picked: Option<i64>) {
        let mut draft = self.draft.clone();
        draft.status = status.into();
        draft.last_picked = last_picked;
        stub.json(&format!("/v1/draft/{DRAFT_ID}"), &draft);
    }
}

pub fn pick(pick_no: u32, slot: u32, player_id: &str) -> Pick {
    Pick {
        round: (pick_no - 1) / 2 + 1,
        pick_no,
        draft_slot: slot,
        player_id: player_id.into(),
        picked_by: Some(if slot == 1 { MY_USER } else { RIVAL_USER }.into()),
        metadata: None,
        is_keeper: None,
    }
}

// ---------- scratch space and the stub CLI ----------

pub fn scratch_dir(label: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-test-{label}-{}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A shell script standing in for `claude`: streams `answer` in two chunks
/// as `stream-json` would, then the result line with a fixed cost.
pub fn stub_claude(dir: &std::path::Path, answer: &str) -> PathBuf {
    let path = dir.join("claude-stub");
    let (head, tail) = answer.split_at(answer.len() / 2);
    let script = format!(
        "#!/bin/sh\ncat >/dev/null\n\
         printf '%s\\n' '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"{head}\"}}}}}}'\n\
         printf '%s\\n' '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"{tail}\"}}}}}}'\n\
         printf '%s\\n' '{{\"type\":\"result\",\"is_error\":false,\"result\":\"{answer}\",\"duration_ms\":1200,\"total_cost_usd\":0.05,\"usage\":{{\"input_tokens\":900,\"output_tokens\":12}}}}'\n"
    );
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}
