//! The one model call, on whichever route was resolved.
//!
//! Split out of `commands_chat.rs` for the line cap, and so a test can hold
//! both ends of it: the route is a plain value a test can build, with a CLI
//! path of its own choosing, rather than something only `find_cli` and a real
//! machine can produce.

use crate::chat::{self, ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_cli;
use crate::chat_client::{self, CancelSignal};
use crate::chat_context::SplitContext;
use std::path::PathBuf;
use std::sync::Arc;

/// Everything the call itself needs, once the provider has been chosen.
pub(crate) struct Route {
    /// "api" or "claude_code": [`super::resolve_provider`]'s answer.
    pub(crate) provider: &'static str,
    /// Where the Claude Code CLI was found, if it was.
    pub(crate) cli: Option<PathBuf>,
    pub(crate) api_key: Option<String>,
    pub(crate) model: ChatModel,
    pub(crate) effort: Effort,
    pub(crate) context: SplitContext,
    pub(crate) messages: Vec<ChatMessage>,
}

impl Route {
    /// Ask, racing `cancel` on either route.
    ///
    /// The signal used to reach only the API route, so on a Mac with Claude
    /// Code installed and no API key, which is what the app picks by itself,
    /// the Cancel button stopped nothing: `cancel_claude` still said it had
    /// found the claim, and the panel sat on "Thinking" until the CLI's own
    /// four-minute deadline. Both routes race it now, and both hand back a
    /// reply marked cut short.
    /// `progress` is where the answer's text goes while it is still
    /// arriving. The API route and the Claude Code route both report it; the
    /// Codex route waits for its process to exit and reads the output in one
    /// go, so there is nothing to hand over until there is everything.
    pub(crate) async fn call(
        self,
        cancel: Arc<CancelSignal>,
        progress: Option<chat::OnProgress>,
    ) -> Result<ChatReply, chat::ChatError> {
        if self.provider == "codex" {
            let cli = self.cli.ok_or_else(|| {
                "Codex CLI not found. Install Codex and run `codex login` once.".to_string()
            })?;
            crate::chat_codex::ask(
                &cli,
                self.model,
                self.effort,
                &self.context.joined(),
                &self.messages,
                cancel,
            )
            .await
            .map_err(chat::ChatError::from)
        } else if self.provider == super::PROVIDER_CLI {
            let cli = self.cli.ok_or_else(|| {
                "Claude Code CLI not found: install it or add an API key".to_string()
            })?;
            chat_cli::ask(
                &cli,
                self.model,
                self.effort,
                &self.context.joined(),
                &self.messages,
                cancel,
                progress,
            )
            .await
            .map_err(chat::ChatError::from)
        } else {
            let api_key = self
                .api_key
                .ok_or_else(|| "no Anthropic API key set: add one in Settings".to_string())?;
            chat::ask_watched(
                chat::Question {
                    // Not the Sleeper client: its eight-second budget cut off
                    // every answer that took longer than a board refresh.
                    http: &chat_client::client(),
                    api_key: &api_key,
                    model: self.model,
                    effort: self.effort,
                    context: &self.context,
                    messages: &self.messages,
                },
                cancel,
                progress,
            )
            .await
        }
    }
}
