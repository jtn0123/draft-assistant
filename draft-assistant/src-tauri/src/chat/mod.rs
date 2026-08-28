//! Ask Claude about the live draft, by shelling out to the locally installed
//! `claude` CLI.
//!
//! The CLI is used rather than the Messages API because it is already
//! authenticated on this machine and needs no API key. `prompt` builds what
//! the model sees (draft state, the whole board as a compact table, and the
//! conversation so far); `cli` resolves and runs the binary and reads its
//! JSON result. This file holds the request/reply types and the two
//! operations the desktop layer exposes: `ask` and `compact`.

mod cli;
mod prompt;

use crate::view::DraftView;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub use cli::ChatUsage;

/// One line of the conversation as the panel shows it: `you`, `claude`, or
/// `summary` — the stand-in for turns that were compacted away.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub text: String,
}

/// Per-question knobs chosen in the panel. Every field has a safe default so
/// an older frontend sending nothing still works.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatOptions {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub fast: bool,
    #[serde(default)]
    pub web_search: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub answer: String,
    pub usage: ChatUsage,
}

/// With the whole board in context (~42k tokens) a detailed Opus answer
/// measured 36s at default effort. The panel offers Cancel, so these bound a
/// hung process rather than pace the UI. Web search adds round trips of its
/// own; compaction reads a long thread and is warned as "a minute or two".
const ASK_TIMEOUT: Duration = Duration::from_secs(90);
const ASK_WITH_WEB_TIMEOUT: Duration = Duration::from_secs(150);
const COMPACT_TIMEOUT: Duration = Duration::from_secs(180);

fn validate_question(question: &str) -> Result<&str, String> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        return Err("Ask a question first".into());
    }
    Ok(trimmed)
}

/// Ask Claude a question about the current draft, in the context of the
/// conversation so far.
pub async fn ask(
    view: &DraftView,
    question: &str,
    history: &[ChatTurn],
    options: &ChatOptions,
) -> Result<ChatReply, String> {
    let question = validate_question(question)?;
    let prompt = prompt::build_prompt(view, history, question)?;
    let timeout = if options.web_search {
        ASK_WITH_WEB_TIMEOUT
    } else {
        ASK_TIMEOUT
    };
    let (answer, usage) = cli::run(&cli::Request {
        prompt: &prompt,
        system_prompt: &prompt::system_prompt(options.web_search),
        options,
        timeout,
    })
    .await?;
    Ok(ChatReply { answer, usage })
}

/// Fold the conversation into one summary turn so a long thread stops
/// costing its full length on every question. Uses no tools and the draft
/// state is not needed: the summary is about what was said.
pub async fn compact(history: &[ChatTurn], options: &ChatOptions) -> Result<ChatReply, String> {
    if history.iter().all(|t| t.role == "summary") {
        return Err("Nothing to compact yet".into());
    }
    let options = ChatOptions {
        web_search: false,
        ..options.clone()
    };
    let (answer, usage) = cli::run(&cli::Request {
        prompt: &prompt::compact_prompt(history),
        system_prompt: prompt::COMPACT_SYSTEM_PROMPT,
        options: &options,
        timeout: COMPACT_TIMEOUT,
    })
    .await?;
    Ok(ChatReply { answer, usage })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_questions_are_rejected() {
        assert!(validate_question("   \n ").is_err());
        assert_eq!(validate_question("  who? ").unwrap(), "who?");
    }

    #[tokio::test]
    async fn compacting_only_a_summary_is_refused_before_any_cli_call() {
        let history = vec![ChatTurn {
            role: "summary".into(),
            text: "earlier".into(),
        }];
        let err = compact(&history, &ChatOptions::default())
            .await
            .unwrap_err();
        assert!(err.contains("Nothing to compact"), "{err}");
    }
}
