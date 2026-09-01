//! League-member wire types from `/league/{id}/users`.
//!
//! Split out of `sleeper.rs` for size; that module re-exports them, so
//! callers keep importing from `crate::sleeper`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeagueUserMeta {
    /// Custom team name. Users who never set one have no key here.
    #[serde(default)]
    pub team_name: Option<String>,
    /// Custom team picture, as a full sleepercdn URL.
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueUser {
    pub user_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Sleeper avatar hash for the account itself.
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub metadata: Option<LeagueUserMeta>,
}

impl LeagueUser {
    /// What to call this team: their custom name, else their handle.
    pub fn label(&self) -> Option<String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.team_name.clone())
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.display_name.clone())
    }

    /// The picture to draw for this team: their custom team image when they
    /// uploaded one, else their account avatar. `None` for the default egg.
    pub fn avatar_ref(&self) -> Option<String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.avatar.clone())
            .filter(|a| !a.trim().is_empty())
            .or_else(|| self.avatar.clone())
            .filter(|a| !a.trim().is_empty())
    }
}
