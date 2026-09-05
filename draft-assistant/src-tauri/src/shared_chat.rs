//! The one chat thread the host and every paired device see.
//!
//! A thread per league per screen, kept on disk beside the rest of the app's
//! data so it survives the server being turned off and the app being closed.
//! Only one question is in flight at a time: the `busy` flag is what a second
//! asker is refused by, and what the phone greys its composer out on.

use crate::chat::{ChatMessage, ChatReply};
use crate::companion::hub::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::Mutex;

/// Who asked. Deliberately not a device id: the thread is read by people.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryDevice {
    pub name: String,
    /// "phone", "desktop", or "host" for the machine running the server.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedEntry {
    pub id: String,
    pub at_ms: u64,
    /// The device that asked — on the answer as well as the question, so the
    /// phone can show whose question it was that came back.
    pub device: EntryDevice,
    /// "user" or "assistant".
    pub role: String,
    pub text: String,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedChatThread {
    pub league_id: String,
    pub screen: String,
    pub busy: bool,
    pub entries: Vec<SharedEntry>,
}

/// Both screens of one league, as they sit on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredLeagueChat {
    #[serde(default)]
    draft: Vec<SharedEntry>,
    #[serde(default)]
    season: Vec<SharedEntry>,
}

impl StoredLeagueChat {
    fn screen(&self, screen: &str) -> &Vec<SharedEntry> {
        match screen {
            "season" => &self.season,
            _ => &self.draft,
        }
    }

    fn screen_mut(&mut self, screen: &str) -> &mut Vec<SharedEntry> {
        match screen {
            "season" => &mut self.season,
            _ => &mut self.draft,
        }
    }
}

/// How many entries one screen's thread keeps. Older ones fall off the front:
/// the file is read whole on every append, and an unbounded draft-night thread
/// would grow until it was slow to write.
const MAX_ENTRIES: usize = 200;

/// The longest question a device may post.
pub const MAX_QUESTION_BYTES: usize = 4_000;

/// Why a question was not accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum PostError {
    /// Another question is still being answered.
    Busy,
    /// Empty, or longer than [`MAX_QUESTION_BYTES`].
    BadText(String),
}

pub struct SharedChat {
    data_dir: PathBuf,
    /// league_id -> both screens, plus which screens are mid-answer. Loaded
    /// from disk the first time a league is touched.
    inner: Mutex<HashMap<String, LiveLeagueChat>>,
}

#[derive(Default)]
struct LiveLeagueChat {
    stored: StoredLeagueChat,
    busy_draft: bool,
    busy_season: bool,
}

impl LiveLeagueChat {
    fn busy(&self, screen: &str) -> bool {
        match screen {
            "season" => self.busy_season,
            _ => self.busy_draft,
        }
    }

    fn set_busy(&mut self, screen: &str, busy: bool) {
        match screen {
            "season" => self.busy_season = busy,
            _ => self.busy_draft = busy,
        }
    }
}

impl SharedChat {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn path_for(&self, league_id: &str) -> PathBuf {
        self.data_dir.join(format!(
            "shared_chat_{}.json",
            crate::cache::safe_key(league_id)
        ))
    }

    /// The thread as it stands, reading it off disk the first time.
    pub async fn thread(&self, league_id: &str, screen: &str) -> SharedChatThread {
        let mut guard = self.inner.lock().await;
        let live = load_into(&mut guard, &self.path_for(league_id), league_id);
        SharedChatThread {
            league_id: league_id.to_string(),
            screen: screen.to_string(),
            busy: live.busy(screen),
            entries: live.stored.screen(screen).clone(),
        }
    }

    /// Append a question and mark the screen busy. The caller runs the answer
    /// and must call [`SharedChat::finish`] however it turns out.
    pub async fn post(
        &self,
        league_id: &str,
        screen: &str,
        device: EntryDevice,
        text: &str,
    ) -> Result<(String, SharedChatThread), PostError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PostError::BadText("that question was empty".to_string()));
        }
        if text.len() > MAX_QUESTION_BYTES {
            return Err(PostError::BadText(format!(
                "that question is too long ({} characters) — the limit is {MAX_QUESTION_BYTES}",
                text.len()
            )));
        }
        let mut guard = self.inner.lock().await;
        let path = self.path_for(league_id);
        let live = load_into(&mut guard, &path, league_id);
        if live.busy(screen) {
            return Err(PostError::Busy);
        }
        let entry = SharedEntry {
            id: entry_id(),
            at_ms: now_ms(),
            device,
            role: "user".to_string(),
            text: text.to_string(),
            cost_usd: None,
            error: None,
        };
        let id = entry.id.clone();
        push(live.stored.screen_mut(screen), entry);
        live.set_busy(screen, true);
        let thread = SharedChatThread {
            league_id: league_id.to_string(),
            screen: screen.to_string(),
            busy: true,
            entries: live.stored.screen(screen).clone(),
        };
        save(&path, &live.stored);
        Ok((id, thread))
    }

    /// Append the answer — or the failure, which is an entry too — and let the
    /// next question through.
    pub async fn finish(
        &self,
        league_id: &str,
        screen: &str,
        device: EntryDevice,
        reply: Result<ChatReply, String>,
    ) -> SharedChatThread {
        let mut guard = self.inner.lock().await;
        let path = self.path_for(league_id);
        let live = load_into(&mut guard, &path, league_id);
        let entry = match reply {
            Ok(reply) => SharedEntry {
                id: entry_id(),
                at_ms: now_ms(),
                device,
                role: "assistant".to_string(),
                text: reply.text,
                cost_usd: Some(reply.cost_usd),
                error: None,
            },
            Err(error) => SharedEntry {
                id: entry_id(),
                at_ms: now_ms(),
                device,
                role: "assistant".to_string(),
                text: String::new(),
                cost_usd: None,
                error: Some(error),
            },
        };
        push(live.stored.screen_mut(screen), entry);
        live.set_busy(screen, false);
        let thread = SharedChatThread {
            league_id: league_id.to_string(),
            screen: screen.to_string(),
            busy: false,
            entries: live.stored.screen(screen).clone(),
        };
        save(&path, &live.stored);
        thread
    }

    /// The thread as the model should see it: the questions and the answers
    /// that worked, in order, roles alternating.
    pub async fn messages(&self, league_id: &str, screen: &str) -> Vec<ChatMessage> {
        let thread = self.thread(league_id, screen).await;
        alternating(&thread.entries)
    }

    /// Empty one screen's thread.
    ///
    /// The only way to start over. The thread keeps two hundred entries, every
    /// paired device adds to the same one, and a phone has no saved-chats
    /// picker to open a new one from — so before this, a thread that had gone
    /// somewhere unhelpful stayed there until somebody deleted the file.
    ///
    /// `busy` is deliberately left as it was: an answer may still be running,
    /// and [`SharedChat::finish`] is what clears the flag. Its entry lands in
    /// the emptied thread, where [`alternating`] leaves it out of the next
    /// request for opening on an assistant turn.
    pub async fn reset(&self, league_id: &str, screen: &str) -> SharedChatThread {
        let mut guard = self.inner.lock().await;
        let path = self.path_for(league_id);
        let live = load_into(&mut guard, &path, league_id);
        live.stored.screen_mut(screen).clear();
        let thread = SharedChatThread {
            league_id: league_id.to_string(),
            screen: screen.to_string(),
            busy: live.busy(screen),
            entries: Vec::new(),
        };
        save(&path, &live.stored);
        thread
    }

    /// Forget everything held in memory, so the next read comes off disk.
    /// Only the tests use this; the app reloads by restarting.
    #[cfg(test)]
    pub async fn forget(&self) {
        self.inner.lock().await.clear();
    }
}

/// The entries as an alternating conversation.
///
/// A failed turn used to leave its question behind: the error entry was
/// filtered out and the question that caused it was not, so the next request
/// carried two user turns in a row and the API refused it. One failure — a
/// missing key, a timeout — broke that league's shared thread for good, and
/// nothing on the phone could clear it. A question whose answer failed goes
/// out with the answer, and any two entries of the same role in a row collapse
/// to the later one, which is the one with an answer still to come.
///
/// The conversation also never opens on an assistant turn, which is a 400.
pub(crate) fn alternating(entries: &[SharedEntry]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    for entry in entries {
        if entry.error.is_some() || entry.text.trim().is_empty() {
            continue;
        }
        if out.last().is_some_and(|last| last.role == entry.role) {
            out.pop();
        }
        if out.is_empty() && entry.role != "user" {
            continue;
        }
        out.push(ChatMessage {
            role: entry.role.clone(),
            content: entry.text.clone(),
        });
    }
    out
}

/// The league's threads, read off disk if this is the first time.
fn load_into<'a>(
    map: &'a mut HashMap<String, LiveLeagueChat>,
    path: &std::path::Path,
    league_id: &str,
) -> &'a mut LiveLeagueChat {
    map.entry(league_id.to_string()).or_insert_with(|| {
        let stored = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredLeagueChat>(&raw).ok())
            .unwrap_or_default();
        LiveLeagueChat {
            stored,
            ..Default::default()
        }
    })
}

fn push(entries: &mut Vec<SharedEntry>, entry: SharedEntry) {
    entries.push(entry);
    if entries.len() > MAX_ENTRIES {
        let extra = entries.len() - MAX_ENTRIES;
        entries.drain(..extra);
    }
}

/// Write the league's threads through a temp file, as everything else in the
/// data directory is written. A failed write is logged, not raised: the answer
/// is already in memory and on its way to the screens, and losing the last
/// turn of history is not a reason to fail the question.
fn save(path: &std::path::Path, stored: &StoredLeagueChat) {
    let json = match serde_json::to_string_pretty(stored) {
        Ok(json) => json,
        Err(e) => {
            crate::applog::warn(format!(
                "could not prepare the shared chat to be saved: {e}"
            ));
            return;
        }
    };
    let tmp = crate::cache::temp_sibling(path);
    if let Err(e) = crate::cache::replace_file(tmp, path.to_path_buf(), json) {
        crate::applog::warn(format!("could not save the shared chat: {e}"));
    }
}

/// An id unique within a thread: the millisecond plus a short random tail, so
/// two questions posted in the same millisecond still differ.
fn entry_id() -> String {
    let tail = crate::companion::rand::bytes(6)
        .map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>())
        .unwrap_or_else(|_| "000000000000".to_string());
    format!("{}-{tail}", now_ms())
}

#[cfg(test)]
#[path = "shared_chat_tests.rs"]
mod tests;
