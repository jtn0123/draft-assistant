//! Keepers noticed during a draft are remembered across launches, so a keeper
//! stays a keeper once the draft has passed its slot.
//!
//! `picks::keeper_pick_nos` can only tell a keeper from a drafted player while
//! the pick still sits *ahead* of the clock. Once the draft rolls past it the
//! evidence is gone — and Sleeper's own `is_keeper` flag is missing on plenty
//! of real keepers — so the judgement is written down the first time it is
//! made and never revisited.

use crate::engine::{Engine, LoadedLeague};
use crate::picks;
use std::collections::HashSet;

/// Read/write the keeper set for a draft. Declared here rather than on
/// `Engine` so the whole of keeper handling is one file, in the style of
/// `SeasonLoader` and `HistoryStore`.
pub trait KeeperStore {
    fn load_keepers(&self, draft_id: &str) -> HashSet<u32>;
    fn save_keepers(&self, draft_id: &str, keepers: &HashSet<u32>) -> Result<(), String>;
}

fn cache_name(draft_id: &str) -> String {
    format!("keepers_{}.json", crate::cache::safe_key(draft_id))
}

impl KeeperStore for Engine {
    fn load_keepers(&self, draft_id: &str) -> HashSet<u32> {
        self.read_cache_any::<Vec<u32>>(&cache_name(draft_id))
            .map(|(_, list)| list.into_iter().collect())
            .unwrap_or_default()
    }

    fn save_keepers(&self, draft_id: &str, keepers: &HashSet<u32>) -> Result<(), String> {
        // Sorted so the file is stable between writes and readable by hand.
        let mut sorted: Vec<u32> = keepers.iter().copied().collect();
        sorted.sort_unstable();
        self.write_cache_checked(&cache_name(draft_id), &sorted)?;
        Ok(())
    }
}

/// Fold newly seen keepers into the league's memory of them: judged from
/// where each pick sits now, and never forgotten once judged.
///
/// A failure to write is a warning, not an error: the app works perfectly
/// well tonight from the in-memory set, and only forgets at the next launch.
pub fn note_keepers(engine: &impl KeeperStore, loaded: &mut LoadedLeague) -> Option<String> {
    let keepers = merge_keepers(loaded)?;
    engine
        .save_keepers(&loaded.draft.draft_id, &keepers)
        .err()
        .map(|error| format!("keepers not saved: {error}"))
}

/// The in-memory half of `note_keepers`: fold what this feed shows into the
/// league's set and hand back the set to write down, or `None` when nothing
/// was learned and there is nothing to write.
///
/// Separate from the write so the poll loop can do this part under the
/// `loaded` lock and the disk part without it.
pub fn merge_keepers(loaded: &mut LoadedLeague) -> Option<HashSet<u32>> {
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    let seen = picks::keeper_pick_nos(&loaded.api_picks, teams, rounds);
    let before = loaded.keeper_pick_nos.len();
    loaded.keeper_pick_nos.extend(seen);
    (loaded.keeper_pick_nos.len() != before).then(|| loaded.keeper_pick_nos.clone())
}

/// Every keeper this league knows about: the remembered set plus whatever the
/// current feed still shows sitting ahead of the clock.
pub fn known_keepers(loaded: &LoadedLeague, teams: u32, rounds: u32) -> HashSet<u32> {
    let mut keepers = loaded.keeper_pick_nos.clone();
    keepers.extend(picks::keeper_pick_nos(&loaded.api_picks, teams, rounds));
    keepers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::now_secs;
    use std::path::PathBuf;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "draft-assistant-{label}-{}-{}",
            std::process::id(),
            now_secs()
        ))
    }

    #[test]
    fn a_keeper_set_survives_a_round_trip_through_the_cache() {
        let dir = test_dir("keeper-store");
        let engine = Engine::new(dir.clone());
        assert!(engine.load_keepers("draft-1").is_empty());

        let keepers: HashSet<u32> = [177, 11, 20].into_iter().collect();
        engine.save_keepers("draft-1", &keepers).unwrap();
        assert_eq!(engine.load_keepers("draft-1"), keepers);
        // Kept per draft, not per app.
        assert!(engine.load_keepers("draft-2").is_empty());

        std::fs::remove_dir_all(dir).unwrap();
    }
}
