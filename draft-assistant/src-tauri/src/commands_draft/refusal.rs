//! When a refused `/picks` answer stops being refused.
//!
//! A pick list that comes back empty mid-draft, or with a hole behind the
//! clock, is almost always a lost or partial response, and the tick keeps
//! the board it has rather than moving the clock backwards. That guard had
//! no exit: a hole Sleeper kept reporting (a pick the commissioner really
//! did remove, a row the API had genuinely lost) was refused on every tick
//! for the rest of the night, and the board froze on the last answer it had
//! believed. So the same answer is refused only [`REFUSAL_LIMIT`] times in a
//! row; the next identical one is adopted, with a warning saying so.
//!
//! The memory is shared by the poll loop and the manual re-pull, and keyed
//! by draft, so a league switch starts the count over.

use super::tick::{picks_rewound, EMPTY_PICKS};
use crate::engine::LoadedLeague;
use crate::sleeper::Pick;
use crate::{keepers, picks};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// How many times in a row the same answer is refused before it is believed.
pub(super) const REFUSAL_LIMIT: u32 = 3;

/// Why one answer is not being put on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// An empty list for a draft that had picks.
    Empty,
    /// A list missing this pick, with later picks still in it.
    Hole(u32),
}

impl Refusal {
    /// The tick error the badge and the toast show.
    pub(super) fn message(&self) -> String {
        match self {
            Refusal::Empty => EMPTY_PICKS.to_string(),
            Refusal::Hole(hole) => picks_rewound(*hole),
        }
    }

    /// The warning logged when the answer is adopted after all.
    fn adopted_note(&self) -> String {
        let what = match self {
            Refusal::Empty => "an empty pick list".to_string(),
            Refusal::Hole(hole) => format!("a pick list without pick {hole}"),
        };
        format!(
            "{what} has come back {} times in a row: adopting it as the platform reports it",
            REFUSAL_LIMIT + 1
        )
    }
}

/// What this answer would be refused for, judged against the board on screen.
pub(super) fn refusal_for(loaded: &LoadedLeague, picks: &[Pick]) -> Option<Refusal> {
    if picks.is_empty() {
        return (!loaded.api_picks.is_empty()).then_some(Refusal::Empty);
    }
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    let keepers = keepers::known_keepers(loaded, teams, rounds);
    picks::rewound_to(&loaded.api_picks, picks, teams, rounds, &keepers).map(Refusal::Hole)
}

/// What to do with one answer.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Put it on the board. Carries the warning to log when it is an answer
    /// that had been refused until now.
    Adopt(Option<String>),
    /// Keep the board; this is the tick's error.
    Refuse(String),
}

/// How many times in a row the same answer has been refused for one draft.
#[derive(Debug, Default)]
pub(super) struct RefusalMemory {
    draft_id: String,
    last: Option<Refusal>,
    streak: u32,
}

impl RefusalMemory {
    /// Judge one answer for `draft_id`: refused for the reason given, up to
    /// the limit, or adopted.
    pub(super) fn judge(&mut self, draft_id: &str, refusal: Option<Refusal>) -> Verdict {
        if self.draft_id != draft_id {
            self.draft_id = draft_id.to_string();
            self.last = None;
            self.streak = 0;
        }
        let Some(refusal) = refusal else {
            self.last = None;
            self.streak = 0;
            return Verdict::Adopt(None);
        };
        if self.last.as_ref() == Some(&refusal) {
            self.streak += 1;
        } else {
            self.last = Some(refusal.clone());
            self.streak = 1;
        }
        if self.streak > REFUSAL_LIMIT {
            self.last = None;
            self.streak = 0;
            return Verdict::Adopt(Some(refusal.adopted_note()));
        }
        Verdict::Refuse(refusal.message())
    }
}

/// The one memory the poll loop and `refresh_picks` share.
///
/// Process-wide rather than on `AppState` because both callers already hold
/// the `loaded` lock when they judge, and the count belongs to the draft,
/// not to whichever of them happened to see the answer.
pub(super) fn shared() -> MutexGuard<'static, RefusalMemory> {
    static MEMORY: OnceLock<Mutex<RefusalMemory>> = OnceLock::new();
    MEMORY
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(pick_no: u32) -> Pick {
        Pick {
            round: 1,
            pick_no,
            draft_slot: 1,
            player_id: format!("p{pick_no}"),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    fn league_at(picks: &[u32]) -> LoadedLeague {
        let mut loaded = crate::keepers::bare_league("draft-hole");
        loaded.api_picks = picks.iter().copied().map(pick).collect();
        loaded
    }

    #[test]
    fn a_hole_behind_the_clock_is_a_refusal_and_a_full_list_is_not() {
        let loaded = league_at(&[1, 2, 3, 4]);
        let holed: Vec<Pick> = [1, 3, 4].into_iter().map(pick).collect();
        assert_eq!(refusal_for(&loaded, &holed), Some(Refusal::Hole(2)));
        let full: Vec<Pick> = [1, 2, 3, 4, 5].into_iter().map(pick).collect();
        assert_eq!(refusal_for(&loaded, &full), None);
        assert_eq!(refusal_for(&loaded, &[]), Some(Refusal::Empty));
        assert_eq!(refusal_for(&league_at(&[]), &[]), None);
    }

    /// The freeze this ends: the same hole, refused forever.
    #[test]
    fn the_same_hole_is_refused_three_times_and_then_believed() {
        let mut memory = RefusalMemory::default();
        for _ in 0..REFUSAL_LIMIT {
            assert!(matches!(
                memory.judge("d1", Some(Refusal::Hole(2))),
                Verdict::Refuse(message) if message.contains("without pick 2")
            ));
        }
        match memory.judge("d1", Some(Refusal::Hole(2))) {
            Verdict::Adopt(Some(note)) => {
                assert!(note.contains("without pick 2"), "{note}");
                assert!(note.contains("4 times in a row"), "{note}");
            }
            other => panic!("the fourth identical answer must be adopted: {other:?}"),
        }
        // Believed once, the count starts over rather than staying open.
        assert!(matches!(
            memory.judge("d1", Some(Refusal::Hole(2))),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn a_different_hole_starts_the_count_again() {
        let mut memory = RefusalMemory::default();
        memory.judge("d1", Some(Refusal::Hole(2)));
        memory.judge("d1", Some(Refusal::Hole(2)));
        memory.judge("d1", Some(Refusal::Hole(5)));
        assert!(matches!(
            memory.judge("d1", Some(Refusal::Hole(5))),
            Verdict::Refuse(_)
        ));
        assert!(matches!(
            memory.judge("d1", Some(Refusal::Hole(5))),
            Verdict::Refuse(_)
        ));
        assert!(matches!(
            memory.judge("d1", Some(Refusal::Hole(5))),
            Verdict::Adopt(Some(_))
        ));
    }

    #[test]
    fn a_good_answer_in_between_forgives_the_streak() {
        let mut memory = RefusalMemory::default();
        memory.judge("d1", Some(Refusal::Empty));
        memory.judge("d1", Some(Refusal::Empty));
        assert_eq!(memory.judge("d1", None), Verdict::Adopt(None));
        for _ in 0..REFUSAL_LIMIT {
            assert!(matches!(
                memory.judge("d1", Some(Refusal::Empty)),
                Verdict::Refuse(_)
            ));
        }
    }

    #[test]
    fn a_league_switch_starts_the_count_over() {
        let mut memory = RefusalMemory::default();
        memory.judge("d1", Some(Refusal::Hole(2)));
        memory.judge("d1", Some(Refusal::Hole(2)));
        memory.judge("d1", Some(Refusal::Hole(2)));
        assert!(matches!(
            memory.judge("d2", Some(Refusal::Hole(2))),
            Verdict::Refuse(_)
        ));
    }
}
