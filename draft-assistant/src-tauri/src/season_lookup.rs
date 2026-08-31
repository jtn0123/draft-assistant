//! Name, position, team and injury status for a player id — the one primitive
//! every section of the season view needs.
//!
//! Two dictionaries back it: the draft board (rich, but only players who were
//! draftable) and Sleeper's full player metadata (thin, but complete — DEF
//! entries live only there). The board wins when both have an answer.

use crate::engine::LoadedLeague;
use crate::season_injury::PlayerFacts;

/// Player facts read off the loaded league: board first, then the player
/// dictionary — DEF entries live only in the latter.
pub struct Lookup<'a> {
    pub loaded: &'a LoadedLeague,
}

impl Lookup<'_> {
    pub fn position(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return Some(self.loaded.board[i].position.clone());
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.position.clone())
            .filter(|p| !p.is_empty())
    }

    pub fn name(&self, player_id: &str) -> String {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return self.loaded.board[i].name.clone();
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| {
                m.full_name.clone().or_else(|| {
                    match (m.first_name.as_ref(), m.last_name.as_ref()) {
                        (Some(f), Some(l)) => Some(format!("{f} {l}")),
                        _ => None,
                    }
                })
            })
            .unwrap_or_else(|| player_id.to_string())
    }

    pub fn team(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return self.loaded.board[i].team.clone();
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.team.clone())
    }

    /// Sleeper's injury status, as it comes off the player dictionary:
    /// "Questionable", "Out", "IR" and so on. Blank entries read as no status.
    pub fn injury(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            if let Some(status) = self.loaded.board[i].injury_status.clone() {
                return Some(status).filter(|s| !s.trim().is_empty());
            }
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.injury_status.clone())
            .filter(|s| !s.trim().is_empty())
    }
}

impl PlayerFacts for Lookup<'_> {
    fn name(&self, player_id: &str) -> String {
        Lookup::name(self, player_id)
    }
    fn team(&self, player_id: &str) -> Option<String> {
        Lookup::team(self, player_id)
    }
    fn injury_status(&self, player_id: &str) -> Option<String> {
        self.injury(player_id)
    }
}
