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
/// Only a snapshot whose clock has not moved past where it stood when the
/// league was loaded is believed: from then on the gap in front of the clock
/// is a new one, and a `/picks` answer that drops a pick opens a false one.
/// See [`KeeperEvidence`].
///
/// The draft's status is deliberately not consulted. It used to be: a
/// `pre_draft` league believed every snapshot, because keepers arrive right
/// up to the first pick. But Sleeper reports `pre_draft` for a mock draft
/// that is halfway through (the clock banner has a special case for exactly
/// this), so one dropped `/picks` row in a mock draft branded every later
/// pick a keeper, on disk, with the guard switched off. Before a real draft
/// starts the clock sits at pick 1 and never moves past the floor, so the
/// floor rule lets every pre-draft keeper through on its own.
pub fn evidence_for(loaded: &LoadedLeague) -> KeeperEvidence {
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    evidence(
        picks::next_open_pick(&loaded.api_picks, teams, rounds),
        loaded.keeper_pick_nos.floor,
    )
}

/// The rule behind [`evidence_for`], in the two facts it turns on: where the
/// draft's first gap is now, and where it was when the league was loaded.
pub fn evidence(open_pick: Option<u32>, floor: Option<u32>) -> KeeperEvidence {
    if open_pick.unwrap_or(u32::MAX) <= floor.unwrap_or(u32::MAX) {
        KeeperEvidence::Position
    } else {
        KeeperEvidence::FlagOnly
    }
}

/// What the app cannot know about a keeper league it met halfway through,
/// said out loud, or `None` when there is nothing to say.
///
/// Position is the only keeper evidence that works when Sleeper leaves
/// `is_keeper` off, and position only works *ahead* of the clock: a keeper
/// already passed sits in a run of filled picks that looks exactly like the
/// picks the room made. So a first load of a draft already under way finds
/// the keepers still to come and none of the ones behind it, and every number
/// built on the keeper set is then quietly pessimistic: survival odds read
/// off a market pick that is too high, a pick market whose early rounds
/// include picks nobody spent, kept players with no mark against their name.
///
/// Nothing here can recover those keepers; identifying them needs a keeper
/// list the draft feed does not carry. What it can do is stop the numbers
/// reading as certain. Opening the same league before its draft starts, which
/// is the ordinary case, records every keeper and this says nothing.
///
/// `remembered` is what was on disk for this draft before the load: a league
/// this app has seen before already has its judgement and is not guessing.
pub fn unseen_keeper_warning(
    picks: &[crate::sleeper::Pick],
    remembered: &HashSet<u32>,
    open_pick: Option<u32>,
) -> Option<String> {
    let open = open_pick?;
    if open <= 1 || !remembered.is_empty() {
        return None;
    }
    // Only worth saying in a league that plainly has keepers: one sitting
    // ahead of the clock, or one Sleeper flagged.
    let has_keepers = picks
        .iter()
        .any(|pick| pick.is_keeper == Some(true) || pick.pick_no >= open);
    has_keepers.then(|| {
        concat!(
            "this draft was already under way the first time it was opened here, ",
            "so keepers already passed cannot be told from ordinary picks: ",
            "survival odds, the pick market and the wait until your turn may be pessimistic"
        )
        .to_string()
    })
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

/// A fourteen-team, fifteen-round league with nothing on its board, for
/// tests of the code around `LoadedLeague` that need one and do not need
/// players. Sleeper reports such a mock draft as `pre_draft` however far
/// along it is, which is the case the keeper tests turn on.
#[cfg(test)]
pub(crate) fn bare_league(draft_id: &str) -> LoadedLeague {
    let league: crate::sleeper::League = serde_json::from_value(serde_json::json!({
        "league_id": "mock", "name": "Mock", "season": "2026", "status": "pre_draft",
        "total_rosters": 14, "roster_positions": ["RB", "BN"], "scoring_settings": {},
    }))
    .unwrap();
    let draft: crate::sleeper::Draft = serde_json::from_value(serde_json::json!({
        "draft_id": draft_id, "status": "drafting", "type": "snake",
        "settings": {"teams": 14, "rounds": 15},
    }))
    .unwrap();
    let roster_rules = crate::roster::RosterRules::new(&league.roster_positions);
    LoadedLeague {
        league,
        draft,
        user_names: Default::default(),
        user_avatars: Default::default(),
        my_slot: None,
        yahoo_ids: Default::default(),
        board: Default::default(),
        board_index: Default::default(),
        replacement_model: crate::valuation::ReplacementModel {
            demand: Default::default(),
            baseline: Default::default(),
        },
        roster_rules,
        api_picks: Vec::new(),
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        keeper_pick_nos: Default::default(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 0,
        projections_fetched_at: 0,
        weekly_fetched_at: 0,
        warnings: Vec::new(),
        player_meta: Default::default(),
        weekly_points: Default::default(),
        second_opinion_loaded_at: None,
    }
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

    fn plain(pick_no: u32) -> crate::sleeper::Pick {
        crate::sleeper::Pick {
            round: (pick_no - 1) / 14 + 1,
            pick_no,
            draft_slot: 1,
            player_id: format!("p{pick_no}"),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    /// The silence this ends: a fresh install opening a keeper league at pick
    /// forty finds the keepers still to come, cannot see the ones behind the
    /// clock, and presents every number built on the keeper set as fact.
    #[test]
    fn a_keeper_league_met_halfway_through_says_what_it_cannot_see() {
        // Picks 1..=39 made, keepers still in the book at 60 and 177.
        let mut picks: Vec<_> = (1..=39).map(plain).collect();
        picks.push(plain(60));
        picks.push(plain(177));
        let none = HashSet::new();
        let warning = unseen_keeper_warning(&picks, &none, Some(40))
            .expect("a mid-draft first load of a keeper league says so");
        assert!(warning.contains("keepers already passed"), "{warning}");

        // Opened before the draft starts: every keeper is ahead of the clock
        // and there is nothing the app cannot see.
        let keepers_only = vec![plain(11), plain(20), plain(177)];
        assert_eq!(unseen_keeper_warning(&keepers_only, &none, Some(1)), None);

        // A league this app has judged before is not guessing.
        let remembered: HashSet<u32> = [60, 177].into_iter().collect();
        assert_eq!(unseen_keeper_warning(&picks, &remembered, Some(40)), None);

        // A draft with no keepers in it at all has nothing to warn about.
        let ordinary: Vec<_> = (1..=39).map(plain).collect();
        assert_eq!(unseen_keeper_warning(&ordinary, &none, Some(40)), None);

        // A flagged keeper behind the clock is evidence too: the league keeps
        // players even though nothing sits ahead of the clock right now.
        let mut flagged = ordinary.clone();
        flagged[10].is_keeper = Some(true);
        assert!(unseen_keeper_warning(&flagged, &none, Some(40)).is_some());

        // A finished board has no open pick and nothing to say.
        assert_eq!(unseen_keeper_warning(&picks, &none, None), None);
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
        // Keepers keep arriving right up to the first pick, and the clock
        // sits at pick 1 the whole time.
        assert_eq!(evidence(Some(1), None), KeeperEvidence::Position);
        assert_eq!(evidence(Some(1), Some(1)), KeeperEvidence::Position);
        // The snapshot the league was loaded from: the gap is the real one.
        assert_eq!(evidence(Some(12), Some(12)), KeeperEvidence::Position);
        // The draft has moved on. A gap now is a hole in the answer, not a
        // keeper, however far ahead of the clock it looks.
        assert_eq!(evidence(Some(13), Some(12)), KeeperEvidence::FlagOnly);
        assert_eq!(evidence(Some(37), Some(12)), KeeperEvidence::FlagOnly);
        // A finished board has no gap at all.
        assert_eq!(evidence(None, Some(12)), KeeperEvidence::FlagOnly);
        // A fixture nobody loaded a league into believes every gap.
        assert_eq!(evidence(Some(37), None), KeeperEvidence::Position);
    }

    fn drafted(pick_no: u32) -> crate::sleeper::Pick {
        crate::sleeper::Pick {
            round: (pick_no - 1) / 14 + 1,
            pick_no,
            draft_slot: (pick_no - 1) % 14 + 1,
            player_id: format!("p{pick_no}"),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    /// Sleeper reports `pre_draft` for a mock draft that is halfway through.
    /// With the floor guard switched off for that status, a `/picks` answer
    /// missing pick 37 of 50 branded 38..=50 keepers, and `note_keepers`
    /// wrote them to disk where every later launch read them back.
    #[test]
    fn a_dropped_pick_in_a_pre_draft_mock_draft_does_not_brand_the_rest_of_the_board() {
        let mut loaded = bare_league("mock-draft");
        loaded.draft.status = "pre_draft".into();
        // Loaded with 12 picks made: the clock stood at 13.
        loaded.api_picks = (1..=12).map(drafted).collect();
        loaded.keeper_pick_nos.floor = Some(13);
        assert!(
            merge_keepers(&mut loaded).is_none(),
            "nothing sits ahead of the clock"
        );

        // Late in round four the answer drops number 37.
        loaded.api_picks = (1..=50).filter(|n| *n != 37).map(drafted).collect();
        assert_eq!(evidence_for(&loaded), KeeperEvidence::FlagOnly);
        assert!(
            merge_keepers(&mut loaded).is_none(),
            "a hole in a running mock draft is not thirteen keepers: {:?}",
            loaded.keeper_pick_nos.picks
        );
    }
}
