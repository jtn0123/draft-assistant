//! Saved conversations. One JSON file per session under
//! `<data_dir>/chats/<draft_id>/<id>.json`, so a session survives a reload
//! or a relaunch, can be reopened from the panel, and is a plain file the
//! user can read, grep or hand over. The shape is the panel's own turn
//! list; it is written whole on every save, which is one small file per
//! answer.

use crate::engine::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A line of a saved conversation, as the panel shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTurn {
    /// `you`, `claude`, `summary` or `note`.
    pub role: String,
    pub text: String,
    /// The pick an answer was written against, when it was one.
    #[serde(default)]
    pub as_of_pick: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub draft_id: String,
    #[serde(default)]
    pub league_name: String,
    /// Unix seconds.
    pub started_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    /// The first question, clipped — what the session list shows.
    #[serde(default)]
    pub title: String,
    pub turns: Vec<SessionTurn>,
    #[serde(default)]
    pub questions: u32,
    #[serde(default)]
    pub cost_usd: f64,
}

/// What the session list needs, without the turns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub started_at: u64,
    pub updated_at: u64,
    pub questions: u32,
    pub cost_usd: f64,
}

impl From<&ChatSession> for SessionSummary {
    fn from(s: &ChatSession) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            started_at: s.started_at,
            updated_at: s.updated_at,
            questions: s.questions,
            cost_usd: s.cost_usd,
        }
    }
}

/// Only what can safely be a file name: an id or draft id is one path
/// segment, never a traversal.
fn safe_segment(raw: &str, what: &str) -> Result<String, String> {
    let safe: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err(format!("chat session {what} is empty or unusable: {raw:?}"));
    }
    Ok(safe)
}

impl Engine {
    fn sessions_dir(&self, draft_id: &str) -> Result<PathBuf, String> {
        Ok(self
            .data_dir
            .join("chats")
            .join(safe_segment(draft_id, "draft id")?))
    }

    fn session_path(&self, draft_id: &str, id: &str) -> Result<PathBuf, String> {
        Ok(self
            .sessions_dir(draft_id)?
            .join(format!("{}.json", safe_segment(id, "id")?)))
    }

    /// Write the session whole (tmp + rename) and return where it went.
    /// Failure is an error, not a warning: the panel says "saved" only when
    /// the file is really there.
    pub fn save_chat_session(&self, session: &ChatSession) -> Result<String, String> {
        let id = safe_segment(&session.id, "id")?;
        let path = self.session_path(&session.draft_id, &id)?;
        let dir = path.parent().ok_or("session path has no parent")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        // The file carries the id it is listed and reopened by.
        let stored = ChatSession {
            id: id.clone(),
            ..session.clone()
        };
        let json = serde_json::to_string_pretty(&stored)
            .map_err(|e| format!("serialize chat session: {e}"))?;
        let tmp = self.tmp_path(&format!("chat-session-{id}"));
        std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("replace {}: {e}", path.display()))?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Every saved session for a draft, newest activity first. A file that
    /// no longer parses is skipped rather than hiding the rest.
    pub fn list_chat_sessions(&self, draft_id: &str) -> Result<Vec<SessionSummary>, String> {
        let dir = self.sessions_dir(draft_id)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("read {}: {e}", dir.display())),
        };
        let mut sessions: Vec<SessionSummary> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let raw = std::fs::read_to_string(entry.path()).ok()?;
                let session: ChatSession = serde_json::from_str(&raw).ok()?;
                Some(SessionSummary::from(&session))
            })
            .collect();
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then(b.started_at.cmp(&a.started_at))
                .then(b.id.cmp(&a.id))
        });
        Ok(sessions)
    }

    pub fn load_chat_session(&self, draft_id: &str, id: &str) -> Result<ChatSession, String> {
        let path = self.session_path(draft_id, id)?;
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("chat session {id} could not be read: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("chat session {id} is not readable: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_draft_ids_become_single_safe_path_segments() {
        assert_eq!(
            safe_segment("2026-08-28_a1", "id").unwrap(),
            "2026-08-28_a1"
        );
        assert_eq!(safe_segment("../../etc/passwd", "id").unwrap(), "etcpasswd");
        assert!(safe_segment("///", "id").unwrap_err().contains("empty"));
    }

    #[test]
    fn a_summary_is_the_session_without_its_turns() {
        let session = ChatSession {
            id: "s1".into(),
            draft_id: "d1".into(),
            league_name: "L".into(),
            started_at: 10,
            updated_at: 20,
            title: "Who?".into(),
            turns: vec![SessionTurn {
                role: "you".into(),
                text: "Who?".into(),
                as_of_pick: None,
            }],
            questions: 1,
            cost_usd: 0.25,
        };
        let summary = SessionSummary::from(&session);
        assert_eq!(summary.id, "s1");
        assert_eq!(summary.title, "Who?");
        assert_eq!(summary.updated_at, 20);
        assert_eq!(summary.questions, 1);
    }
}
