//! The pick list itself: merging the Sleeper feed with manual picks, where
//! the draft has actually got to, and which picks are keepers.
//!
//! All of it exists because a keeper league does not start with an empty
//! board. Keepers are entered as real picks, scattered all over the draft
//! (round 1 pick 11, round 2 pick 14, round 13 pick 177 …) hours before
//! anybody is on the clock. Every rule here that counts *positions* rather
//! than *picks* is there because counting picks puts such a league several
//! rounds ahead of itself before it begins.

use crate::engine::Engine;
use crate::sleeper::Pick;
use std::collections::HashSet;

/// Read/write the picks the user typed in by hand, when Sleeper's feed is
/// lagging or the draft is happening off-platform. Declared here rather than
/// on `Engine`, in the style of `KeeperStore` and `SeasonLoader`.
pub trait ManualPickStore {
    fn load_manual_picks(&self, draft_id: &str) -> Vec<Pick>;
    fn save_manual_picks(&self, draft_id: &str, picks: &[Pick]) -> Result<(), String>;
}

fn cache_name(draft_id: &str) -> String {
    format!("manual_picks_{}.json", crate::cache::safe_key(draft_id))
}

impl ManualPickStore for Engine {
    fn load_manual_picks(&self, draft_id: &str) -> Vec<Pick> {
        self.read_cache_any(&cache_name(draft_id))
            .map(|(_, picks)| picks)
            .unwrap_or_default()
    }

    fn save_manual_picks(&self, draft_id: &str, picks: &[Pick]) -> Result<(), String> {
        self.write_cache_checked(&cache_name(draft_id), &picks)?;
        Ok(())
    }
}

/// Merge API picks with manual fallback picks. API picks are authoritative;
/// a manual pick survives only where the API has not filled that pick number.
///
/// Keyed on the number rather than "beyond the highest API pick": with
/// keepers already in the book at pick 177, the old rule silently threw away
/// every manual pick the user typed below it.
pub fn merged_picks(api: &[Pick], manual: &[Pick]) -> Vec<Pick> {
    let taken: HashSet<u32> = api.iter().map(|p| p.pick_no).collect();
    let mut picks = api.to_vec();
    for m in manual {
        if !taken.contains(&m.pick_no) {
            picks.push(m.clone());
        }
    }
    picks.sort_by_key(|p| p.pick_no);
    picks
}

/// Drop the manual picks the API has since caught up with — same pick number
/// or same player. Returns whether anything was removed, so the caller knows
/// to write the shortened list back to disk.
pub fn reconcile_manual_picks(api: &[Pick], manual: &mut Vec<Pick>) -> bool {
    let before = manual.len();
    let api_numbers: HashSet<u32> = api.iter().map(|pick| pick.pick_no).collect();
    let api_players: HashSet<&str> = api.iter().map(|pick| pick.player_id.as_str()).collect();
    manual.retain(|pick| {
        !api_numbers.contains(&pick.pick_no) && !api_players.contains(pick.player_id.as_str())
    });
    manual.len() != before
}

/// The lowest pick number nobody has filled yet, or `None` once the board is
/// full — the pick that is genuinely on the clock.
pub fn next_open_pick(picks: &[Pick], teams: u32, rounds: u32) -> Option<u32> {
    let made: HashSet<u32> = picks.iter().map(|p| p.pick_no).collect();
    (1..=teams.saturating_mul(rounds)).find(|pick| !made.contains(pick))
}

/// Which picks are keepers, judged by position rather than by Sleeper's flag:
/// anything flagged, plus anything already in the book at or beyond the next
/// open pick — a pick the draft has not reached yet can only be a keeper.
///
/// Union this into `LoadedLeague::keeper_pick_nos` whenever picks arrive, so
/// the judgement survives the draft passing the slot.
pub fn keeper_pick_nos(picks: &[Pick], teams: u32, rounds: u32) -> HashSet<u32> {
    let open = next_open_pick(picks, teams, rounds).unwrap_or(u32::MAX);
    picks
        .iter()
        .filter(|p| p.is_keeper == Some(true) || p.pick_no >= open)
        .map(|p| p.pick_no)
        .collect()
}

/// How many picks actually have to be *made* between here and `mine`.
///
/// Keepers sitting in between are already in the book and cost nobody any
/// time, so counting pick numbers overstates the wait — in a league with 27
/// keepers it said "24 picks until you" when four were left to happen.
pub fn picks_until(current_pick: u32, mine: u32, picks: &[Pick]) -> u32 {
    let made: HashSet<u32> = picks.iter().map(|p| p.pick_no).collect();
    (current_pick..mine).filter(|p| !made.contains(p)).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(pick_no: u32, player_id: &str) -> Pick {
        Pick {
            round: 1,
            pick_no,
            draft_slot: 1,
            player_id: player_id.into(),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    fn keeper(pick_no: u32) -> Pick {
        Pick {
            is_keeper: Some(true),
            ..pick(pick_no, &format!("kept-{pick_no}"))
        }
    }

    #[test]
    fn a_manual_pick_below_a_keeper_is_not_thrown_away() {
        // Keepers at 11 and 20; the user types pick 1 by hand.
        let api = [keeper(11), keeper(20)];
        let manual = [pick(1, "typed")];
        let merged = merged_picks(&api, &manual);
        assert_eq!(
            merged.iter().map(|p| p.pick_no).collect::<Vec<_>>(),
            vec![1, 11, 20]
        );
        // The API filling that same number wins.
        let merged = merged_picks(&[pick(1, "real"), keeper(11)], &manual);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].player_id, "real");
    }

    #[test]
    fn manual_picks_survive_a_reload() {
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-manual-picks-{}-{}",
            std::process::id(),
            crate::engine::now_secs()
        ));
        let engine = Engine::new(dir.clone());
        let manual = vec![pick(1, "manual-1"), pick(2, "manual-2")];

        engine.save_manual_picks("draft-123", &manual).unwrap();
        let reloaded = engine.load_manual_picks("draft-123");
        assert_eq!(
            reloaded.iter().map(|p| p.pick_no).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(engine.load_manual_picks("draft-999").is_empty());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_keeper_beyond_a_manual_pick_no_longer_wipes_it_out() {
        // The keeper league case: a keeper sits at pick 177 from the moment
        // the draft is created, and the user types pick 1 by hand.
        let mut manual = vec![pick(1, "typed")];
        let api = vec![keeper(177)];
        assert!(!reconcile_manual_picks(&api, &mut manual));
        assert_eq!(manual.len(), 1);
    }

    #[test]
    fn manual_picks_the_api_has_caught_up_with_are_dropped() {
        let api = [pick(1, "real"), keeper(11)];
        let mut manual = vec![pick(1, "typed"), pick(2, "elsewhere"), pick(3, "real")];
        assert!(reconcile_manual_picks(&api, &mut manual));
        // 1 was filled by the API; 3 named a player the API already has.
        assert_eq!(
            manual.iter().map(|p| p.pick_no).collect::<Vec<_>>(),
            vec![2]
        );
        assert!(!reconcile_manual_picks(&api, &mut manual));
    }

    #[test]
    fn the_clock_sits_at_the_first_gap_not_the_pick_count() {
        // Three keepers in the book, nothing drafted: the draft is at pick 1.
        let picks = [keeper(11), keeper(14), keeper(20)];
        assert_eq!(next_open_pick(&picks, 14, 15), Some(1));
        // Fill 1..=10 and it moves to 12, stepping over the keeper at 11.
        let mut picks = picks.to_vec();
        picks.extend((1..=10).map(|n| pick(n, "drafted")));
        assert_eq!(next_open_pick(&picks, 14, 15), Some(12));
        // A full board has no open pick.
        let full: Vec<Pick> = (1..=4).map(|n| pick(n, "x")).collect();
        assert_eq!(next_open_pick(&full, 2, 2), None);
    }

    #[test]
    fn keepers_are_judged_by_position_when_sleepers_flag_is_missing() {
        // Nothing flagged, but 11 and 20 sit beyond the open pick (1).
        let picks = [pick(11, "a"), pick(20, "b")];
        let mut found: Vec<u32> = keeper_pick_nos(&picks, 14, 15).into_iter().collect();
        found.sort_unstable();
        assert_eq!(found, vec![11, 20]);
    }

    #[test]
    fn a_flagged_keeper_the_draft_has_passed_is_still_a_keeper() {
        let mut picks: Vec<Pick> = (1..=10).map(|n| pick(n, "drafted")).collect();
        picks.push(keeper(5));
        picks[4] = keeper(5);
        // Open pick is 11, so position alone would say nothing is a keeper.
        let found = keeper_pick_nos(&picks, 14, 15);
        assert!(found.contains(&5), "flag alone must be enough: {found:?}");
    }

    #[test]
    fn keepers_in_the_way_do_not_count_towards_the_wait() {
        // At pick 12, mine is 20, and 14 is a keeper already in the book.
        let picks = [keeper(14)];
        assert_eq!(picks_until(12, 20, &picks), 7);
        assert_eq!(picks_until(12, 20, &[]), 8);
        assert_eq!(picks_until(12, 12, &[]), 0);
    }
}
