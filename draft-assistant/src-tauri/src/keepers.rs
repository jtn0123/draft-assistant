//! Keepers noticed during a draft are remembered across launches, so a keeper
//! stays a keeper once the draft has passed its slot.
//!
//! `picks::keeper_pick_nos` can only tell a keeper from a drafted player while
//! the pick still sits *ahead* of the clock. Once the draft rolls past it the
//! evidence is gone — and Sleeper's own `is_keeper` flag is missing on plenty
//! of real keepers — so the judgement is written down the first time it is
//! made and never revisited.

use crate::engine::{Engine, LoadedLeague};
use crate::picks::{self, KeeperEvidence};
use std::collections::HashSet;

/// What a loaded league knows about its own keepers.
///
/// The floor lives beside the set because the two are only meaningful
/// together: the set is what has been judged, and the floor is how far the
/// draft had got when it was judged, which is the whole of what decides
/// whether a later snapshot may add to it.
#[derive(Debug, Clone, Default)]
pub struct KeeperMemory {
    /// Pick numbers known to be keepers: flagged by Sleeper, or seen sitting
    /// ahead of the clock at some point. Remembered on disk because a keeper
    /// stays a keeper once the draft passes its slot.
    pub picks: HashSet<u32>,
    /// The pick the clock stood at when the league was loaded, past which a
    /// gap in the pick list is no longer believed to mean "keeper". `None` on
    /// a memory no load has filled in — a test fixture built by hand — which
    /// believes every gap.
    pub floor: Option<u32>,
}

impl KeeperMemory {
    pub fn is_empty(&self) -> bool {
        self.picks.is_empty()
    }

    /// Forget the picks, keeping the floor.
    pub fn clear(&mut self) {
        self.picks.clear();
    }
}

/// Read/write the keeper set for a draft. Declared here rather than on
/// `Engine` so the whole of keeper handling is one file, in the style of
/// `SeasonLoader` and `HistoryStore`.
pub trait KeeperStore {
    fn load_keepers(&self, draft_id: &str) -> HashSet<u32>;
    fn save_keepers(&self, draft_id: &str, keepers: &HashSet<u32>) -> Result<(), String>;
    /// Forget everything this app decided about a draft's keepers.
    ///
    /// The judgement is deliberately never revisited, which is right when it
    /// was right and unfixable when it was wrong — a league branded from a
    /// bad snapshot stayed branded through every relaunch, with nothing on
    /// screen to undo it. This is that undo.
    fn clear_keepers(&self, draft_id: &str) -> Result<(), String>;
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

    fn clear_keepers(&self, draft_id: &str) -> Result<(), String> {
        let path = self.data_dir.join(cache_name(draft_id));
        match std::fs::remove_file(&path) {
            // A draft that never had a keeper file is already clear; saying so
            // as an error would make the button fail on the common case.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other.map_err(|error| format!("keeper file not removed: {error}")),
        }
    }
}

/// How much this league's current pick snapshot is allowed to say about
/// keepers.
///
/// Before the draft starts, position is all there is and keepers arrive right
/// up to the first pick, so every snapshot may widen the set. Once it is
/// running, only the snapshot the league was loaded from is believed: from
/// then on the gap in front of the clock has moved, and a `/picks` answer
/// that drops a pick opens a false one. See [`KeeperEvidence`].
pub fn evidence_for(loaded: &LoadedLeague) -> KeeperEvidence {
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    evidence(
        &loaded.draft.status,
        picks::next_open_pick(&loaded.api_picks, teams, rounds),
        loaded.keeper_pick_nos.floor,
    )
}

/// The rule behind [`evidence_for`], in the three facts it turns on: the
/// draft's status, where its first gap is now, and where the gap was when the
/// league was loaded.
pub fn evidence(status: &str, open_pick: Option<u32>, floor: Option<u32>) -> KeeperEvidence {
    if status == "pre_draft" {
        return KeeperEvidence::Position;
    }
    if open_pick.unwrap_or(u32::MAX) <= floor.unwrap_or(u32::MAX) {
        KeeperEvidence::Position
    } else {
        KeeperEvidence::FlagOnly
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
    let seen = picks::keeper_pick_nos(&loaded.api_picks, teams, rounds, evidence_for(loaded));
    let before = loaded.keeper_pick_nos.picks.len();
    loaded.keeper_pick_nos.picks.extend(seen);
    (loaded.keeper_pick_nos.picks.len() != before).then(|| loaded.keeper_pick_nos.picks.clone())
}

/// Every keeper this league knows about: the remembered set plus whatever the
/// current feed still shows sitting ahead of the clock.
pub fn known_keepers(loaded: &LoadedLeague, teams: u32, rounds: u32) -> HashSet<u32> {
    let mut keepers = loaded.keeper_pick_nos.picks.clone();
    keepers.extend(picks::keeper_pick_nos(
        &loaded.api_picks,
        teams,
        rounds,
        evidence_for(loaded),
    ));
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

    /// A league branded from one bad `/picks` answer stayed branded through
    /// every relaunch, with nothing on screen able to undo it.
    #[test]
    fn clearing_keepers_forgets_them_across_launches_and_never_fails_when_there_are_none() {
        let dir = test_dir("keeper-clear");
        let engine = Engine::new(dir.clone());
        // Clearing a draft that never had a file is a no-op, not an error.
        engine.clear_keepers("draft-1").unwrap();

        engine
            .save_keepers("draft-1", &[11, 20].into_iter().collect())
            .unwrap();
        engine
            .save_keepers("draft-2", &[7].into_iter().collect())
            .unwrap();
        engine.clear_keepers("draft-1").unwrap();
        assert!(engine.load_keepers("draft-1").is_empty());
        // Only the draft that was asked for.
        assert_eq!(engine.load_keepers("draft-2"), [7].into_iter().collect());

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The evidence rule that stops one dropped pick mid-draft branding the
    /// rest of the board, permanently, on disk.
    #[test]
    fn position_stops_counting_once_the_draft_has_moved_past_where_it_was_loaded() {
        // Keepers keep arriving right up to the first pick.
        assert_eq!(
            evidence("pre_draft", Some(1), None),
            KeeperEvidence::Position
        );
        assert_eq!(
            evidence("pre_draft", Some(37), Some(12)),
            KeeperEvidence::Position
        );
        // The snapshot the league was loaded from: the gap is the real one.
        assert_eq!(
            evidence("drafting", Some(12), Some(12)),
            KeeperEvidence::Position
        );
        // The draft has moved on. A gap now is a hole in the answer, not a
        // keeper, however far ahead of the clock it looks.
        assert_eq!(
            evidence("drafting", Some(13), Some(12)),
            KeeperEvidence::FlagOnly
        );
        assert_eq!(
            evidence("drafting", Some(37), Some(12)),
            KeeperEvidence::FlagOnly
        );
        // A finished board has no gap at all.
        assert_eq!(
            evidence("complete", None, Some(12)),
            KeeperEvidence::FlagOnly
        );
        // A paused draft is a running draft that has stopped, not one that
        // has not started.
        assert_eq!(
            evidence("paused", Some(37), Some(12)),
            KeeperEvidence::FlagOnly
        );
        // A fixture nobody loaded a league into believes every gap.
        assert_eq!(
            evidence("drafting", Some(37), None),
            KeeperEvidence::Position
        );
    }
}
