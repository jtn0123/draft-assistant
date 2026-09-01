//! League-member wire types from `/league/{id}/users`.
//!
//! Split out of `sleeper.rs` for size; that module re-exports them, so
//! callers keep importing from `crate::sleeper`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// user_id -> what to call their team, for every member who has a label.
///
/// Both the current league and the previous one need exactly this map, and
/// building it twice is how the two came to disagree about which of a team
/// name and a handle wins.
pub(crate) fn label_map(users: &[LeagueUser]) -> HashMap<String, String> {
    users
        .iter()
        .filter_map(|u| u.label().map(|n| (u.user_id.clone(), n)))
        .collect()
}

/// user_id -> the picture to draw for their team, where they have one.
pub(crate) fn avatar_map(users: &[LeagueUser]) -> HashMap<String, String> {
    users
        .iter()
        .filter_map(|u| u.avatar_ref().map(|a| (u.user_id.clone(), a)))
        .collect()
}
