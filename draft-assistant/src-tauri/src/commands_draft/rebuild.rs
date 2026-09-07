//! What "Refresh projections" keeps from the league it rebuilds.
//!
//! The rebuild goes back to the wire for everything and assembles a new
//! `LoadedLeague`, and that assembly reads the keeper floor off the pick list
//! as it stands at that instant. Halfway through a keeper league that floor
//! sits at the current pick, so the keepers the load had recognised ahead of
//! the old floor were still in the file but every later gap (a dropped row,
//! an undone pick) was now judged from the new, later floor. The judgement
//! that was made at load is carried across instead of being made again.

use crate::keepers::KeeperMemory;

/// Carry the keeper judgement from the league on screen into its rebuilt
/// replacement: the floor the load set, and every keeper noticed since,
/// including the ones a failed save never got to disk.
pub(super) fn carry_keepers(previous: &KeeperMemory, fresh: &mut KeeperMemory) {
    fresh.floor = previous.floor;
    fresh.picks.extend(previous.picks.iter().copied());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_rebuild_keeps_the_floor_the_load_set() {
        let previous = KeeperMemory {
            picks: HashSet::from([3, 7]),
            floor: Some(5),
        };
        // What the rebuild's own assembly produced: the file's keepers and a
        // floor read off a draft that has moved on to pick 40.
        let mut fresh = KeeperMemory {
            picks: HashSet::from([3]),
            floor: Some(40),
        };
        carry_keepers(&previous, &mut fresh);
        assert_eq!(fresh.floor, Some(5), "the floor was re-derived");
        assert_eq!(
            fresh.picks,
            HashSet::from([3, 7]),
            "an unsaved keeper was lost"
        );
    }
}
