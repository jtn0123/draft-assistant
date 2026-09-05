//! What this app remembers between launches: the settings file, and the list
//! of leagues the picker shows.
//!
//! Split out of `engine.rs`, which was at the file cap. Nothing here changed
//! in the move. Every field added after the first release carries
//! `#[serde(default)]`, because a config written by an older build has to keep
//! loading rather than resetting the user's settings on upgrade.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub my_user_id: Option<String>,
    pub active_league_id: Option<String>,
    #[serde(default)]
    pub leagues: Vec<StoredLeague>,
    /// Key for the Ask Claude panel. Stored in the app's own data directory
    /// and never sent anywhere except api.anthropic.com.
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    /// How Ask Claude reaches Claude: "api" (the key above) or "claude_code"
    /// (the Claude Code CLI, signed in with a subscription). Unset means
    /// whichever is available, preferring the CLI when there is no key.
    #[serde(default)]
    pub chat_provider: Option<String>,
    /// Dollars one screen's Ask Claude may spend before the backend refuses
    /// the next turn. `None` means nobody has set one and the default is in
    /// force; `Some(0.0)` means the user turned the cap off.
    #[serde(default)]
    pub chat_budget_usd: Option<f64>,
    /// screen ("draft" / "season") -> what that screen's chats have cost, all
    /// conversations together. The cap is checked against this, so it has to
    /// outlive both the conversation and the app.
    #[serde(default)]
    pub chat_spend_usd: HashMap<String, f64>,
    /// What this Mac calls itself in the shared chat and on a follower's
    /// "Hosted by …" pill. Unset until the user edits it, and then the
    /// machine's own computer name is used.
    #[serde(default)]
    pub device_name: Option<String>,
    /// The port the phone server last took, so a bookmarked URL keeps working.
    #[serde(default)]
    pub companion_port: Option<u16>,
    /// Whether it was on when the app last closed; see COMPANION-API.md.
    #[serde(default)]
    pub companion_enabled: bool,
    /// How much the log writes: `"debug"` or `"info"`. `None` means nobody has
    /// chosen, and `DRAFT_ASSISTANT_DEBUG` decides as it always did. Stored so
    /// that turning verbose logging on to chase something survives the restart
    /// that is usually the next thing the user tries.
    #[serde(default)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLeague {
    pub league_id: String,
    pub name: String,
    pub season: String,
    /// Sleeper's `pre_draft`/`drafting`/`in_season`/`complete`; absent for
    /// older configs and for a mock draft, which has no league to ask.
    #[serde(default)]
    pub status: Option<String>,
    /// `"sleeper"` or `"yahoo"`. Defaulted so a config written before Yahoo
    /// existed still loads, with every league in it read as a Sleeper one —
    /// which is what it was.
    #[serde(default = "sleeper")]
    pub platform: String,
}

/// The platform a stored league has when its config predates the field.
fn sleeper() -> String {
    crate::view_types::SLEEPER.to_string()
}
