//! Saved Ask Claude conversations: written as plain files under the data
//! dir, listed newest first per draft, reopened whole, and never a path
//! traversal.

mod support;

use draft_assistant_lib::app::AppCore;
use draft_assistant_lib::chat::{ChatSession, SessionTurn};
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::sleeper::SleeperClient;

fn core(label: &str) -> AppCore {
    AppCore::new(Engine {
        client: SleeperClient::with_base_url("http://127.0.0.1:1"),
        data_dir: support::scratch_dir(label),
    })
}

fn session(id: &str, draft_id: &str, started_at: u64, title: &str) -> ChatSession {
    ChatSession {
        id: id.into(),
        draft_id: draft_id.into(),
        league_name: "Fixture".into(),
        started_at,
        updated_at: started_at,
        title: title.into(),
        turns: vec![
            SessionTurn {
                role: "you".into(),
                text: title.into(),
                as_of_pick: None,
            },
            SessionTurn {
                role: "claude".into(),
                text: "Take Fixture QB.".into(),
                as_of_pick: Some(1),
            },
        ],
        questions: 1,
        cost_usd: 0.05,
    }
}

#[test]
fn a_session_round_trips_through_a_file_under_the_data_dir() {
    let core = core("sessions-roundtrip");
    let saved = session("s-1", "draft-1", 100, "Who should I take?");
    let path = core.save_chat_session(&saved).unwrap();
    assert!(
        path.ends_with("/chats/draft-1/s-1.json"),
        "one file per session: {path}"
    );
    assert!(std::path::Path::new(&path).is_file());
    let loaded = core.load_chat_session("draft-1", "s-1").unwrap();
    assert_eq!(loaded, saved);
    // Readable by anything: it is pretty JSON with the turns in it.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("\"Take Fixture QB.\""));
    assert!(raw.contains('\n'));
}

#[test]
fn saving_again_replaces_the_file_and_the_list_orders_by_latest_activity() {
    let core = core("sessions-list");
    core.save_chat_session(&session("old", "draft-1", 100, "First"))
        .unwrap();
    core.save_chat_session(&session("new", "draft-1", 200, "Second"))
        .unwrap();
    // The old session gets a new answer: it is the most recent again.
    let mut old = session("old", "draft-1", 100, "First");
    old.updated_at = 300;
    old.questions = 2;
    old.turns.push(SessionTurn {
        role: "you".into(),
        text: "Why?".into(),
        as_of_pick: None,
    });
    core.save_chat_session(&old).unwrap();

    let list = core.list_chat_sessions("draft-1").unwrap();
    let ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["old", "new"]);
    assert_eq!(list[0].questions, 2);
    assert_eq!(list[0].title, "First");
    assert_eq!(
        core.load_chat_session("draft-1", "old")
            .unwrap()
            .turns
            .len(),
        3
    );
    // No scratch files left beside the sessions.
    let dir = core.engine.data_dir.join("chats").join("draft-1");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
}

#[test]
fn sessions_are_kept_per_draft_and_a_draft_with_none_lists_nothing() {
    let core = core("sessions-per-draft");
    core.save_chat_session(&session("a", "draft-1", 1, "A"))
        .unwrap();
    core.save_chat_session(&session("b", "draft-2", 2, "B"))
        .unwrap();
    assert_eq!(core.list_chat_sessions("draft-1").unwrap().len(), 1);
    assert_eq!(core.list_chat_sessions("draft-2").unwrap()[0].id, "b");
    assert!(core.list_chat_sessions("draft-3").unwrap().is_empty());
    assert!(core
        .load_chat_session("draft-1", "b")
        .unwrap_err()
        .contains("could not be read"));
}

#[test]
fn ids_cannot_escape_the_sessions_directory_and_bad_files_are_skipped() {
    let core = core("sessions-safety");
    let mut sneaky = session("../../escape", "draft-1", 1, "Nope");
    let path = core.save_chat_session(&sneaky).unwrap();
    assert!(path.ends_with("/chats/draft-1/escape.json"), "{path}");
    sneaky.id = "///".into();
    assert!(core
        .save_chat_session(&sneaky)
        .unwrap_err()
        .contains("empty or unusable"));
    assert!(core.list_chat_sessions("").is_err());

    // A file that is not a session does not hide the ones that are.
    let dir = core.engine.data_dir.join("chats").join("draft-1");
    std::fs::write(dir.join("broken.json"), "{not json").unwrap();
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let list = core.list_chat_sessions("draft-1").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "escape");
}
