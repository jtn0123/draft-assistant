//! The manual-pick fallback: marking a player taken when Sleeper's API lags or
//! the draft is offline.
//!
//! Pure functions over `LoadedLeague`, so the guards are testable without a
//! Tauri `AppState`. `desktop.rs` wraps them with the save-and-roll-back step.

use crate::draft;
use crate::engine::LoadedLeague;
use crate::sleeper::Pick;
use crate::view::{merged_picks, next_open_pick};

/// Record `player_id` at the next open pick number. Rejects a player already
/// taken (by the API or manually), one not on the board, and a full draft.
pub fn apply_manual_pick(loaded: &mut LoadedLeague, player_id: String) -> Result<(), String> {
    let teams = loaded.draft.settings.teams;
    let (order, _) = draft::DraftOrder::from_draft(&loaded.draft);
    let picks = merged_picks(&loaded.api_picks, &loaded.manual_picks);
    if picks.iter().any(|p| p.player_id == player_id) {
        return Err("player already drafted".into());
    }
    if !loaded.board_index.contains_key(&player_id) {
        return Err(format!("player {player_id} is not on the board"));
    }
    let rounds = loaded.draft.settings.rounds;
    let Some(pick_no) = next_open_pick(&picks, teams, rounds) else {
        return Err("draft is complete".into());
    };
    loaded.manual_picks.push(Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: draft::slot_for_pick(pick_no, teams, order),
        player_id,
        picked_by: None,
        metadata: None,
        is_keeper: None,
    });
    Ok(())
}

/// Remove the newest manual pick and hand it back so a failed save can
/// restore it. API picks are Sleeper's record and cannot be undone here.
pub fn undo_manual_pick(loaded: &mut LoadedLeague) -> Result<Pick, String> {
    loaded
        .manual_picks
        .pop()
        .ok_or_else(|| "no manual picks to undo (API picks cannot be undone locally)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BoardPlayer;
    use crate::roster::RosterRules;
    use crate::sleeper::{Draft, League};
    use crate::valuation::ReplacementModel;
    use serde_json::json;
    use std::collections::HashMap;

    fn board_player(id: &str) -> BoardPlayer {
        BoardPlayer {
            player_id: id.into(),
            name: id.into(),
            position: "WR".into(),
            team: None,
            bye_week: None,
            points: 100.0,
            bonus_points: 0.0,
            vorp: 10.0,
            tier: 1,
            position_rank: 1,
            overall_rank: 1,
            adp: None,
            injury_status: None,
            sleeper_pts_ppr: None,
        }
    }

    /// Two teams, two rounds: four picks in total.
    fn league() -> LoadedLeague {
        let league: League = serde_json::from_value(json!({
            "league_id": "l1", "name": "Test", "season": "2026", "status": "drafting",
            "total_rosters": 2, "roster_positions": ["WR", "BN"], "scoring_settings": {},
            "draft_id": "d1"
        }))
        .unwrap();
        let draft: Draft = serde_json::from_value(json!({
            "draft_id": "d1", "status": "drafting", "type": "snake",
            "settings": {"teams": 2, "rounds": 2}
        }))
        .unwrap();
        let board: Vec<BoardPlayer> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| board_player(id))
            .collect();
        let board_index = board
            .iter()
            .enumerate()
            .map(|(i, p)| (p.player_id.clone(), i))
            .collect();
        LoadedLeague {
            league,
            draft,
            user_names: HashMap::new(),
            board,
            board_index,
            replacement_model: ReplacementModel {
                baseline: HashMap::new(),
                demand: HashMap::new(),
            },
            roster_rules: RosterRules::new(&["WR".into(), "BN".into()]),
            api_picks: Vec::new(),
            manual_picks: Vec::new(),
            keeper_pick_nos: Default::default(),
            poll_last_success_at: None,
            poll_consecutive_failures: 0,
            poll_last_error: None,
            players_fetched_at: 0,
            projections_fetched_at: 0,
            weekly_fetched_at: 0,
            warnings: Vec::new(),
            player_meta: HashMap::new(),
        }
    }

    fn api_pick(pick_no: u32, player_id: &str) -> Pick {
        Pick {
            round: 1,
            pick_no,
            draft_slot: pick_no,
            player_id: player_id.into(),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    #[test]
    fn picks_are_numbered_after_the_api_and_follow_the_snake() {
        let mut loaded = league();
        loaded.api_picks = vec![api_pick(1, "a")];
        apply_manual_pick(&mut loaded, "b".into()).unwrap();
        apply_manual_pick(&mut loaded, "c".into()).unwrap();
        let picks: Vec<(u32, u32, u32)> = loaded
            .manual_picks
            .iter()
            .map(|p| (p.pick_no, p.round, p.draft_slot))
            .collect();
        // Pick 2 is slot 2 in round 1; pick 3 is slot 2 again as the snake turns.
        assert_eq!(picks, vec![(2, 1, 2), (3, 2, 2)]);
    }

    #[test]
    fn a_player_already_taken_by_the_api_or_manually_is_refused() {
        let mut loaded = league();
        loaded.api_picks = vec![api_pick(1, "a")];
        assert_eq!(
            apply_manual_pick(&mut loaded, "a".into()).unwrap_err(),
            "player already drafted"
        );
        apply_manual_pick(&mut loaded, "b".into()).unwrap();
        assert_eq!(
            apply_manual_pick(&mut loaded, "b".into()).unwrap_err(),
            "player already drafted"
        );
        assert_eq!(loaded.manual_picks.len(), 1);
    }

    #[test]
    fn a_player_not_on_the_board_is_refused() {
        let mut loaded = league();
        let err = apply_manual_pick(&mut loaded, "nobody".into()).unwrap_err();
        assert!(err.contains("not on the board"), "{err}");
        assert!(loaded.manual_picks.is_empty());
    }

    #[test]
    fn a_full_draft_takes_no_more_picks() {
        let mut loaded = league();
        for id in ["a", "b", "c", "d"] {
            apply_manual_pick(&mut loaded, id.into()).unwrap();
        }
        assert_eq!(
            apply_manual_pick(&mut loaded, "e".into()).unwrap_err(),
            "draft is complete"
        );
    }

    #[test]
    fn undo_returns_the_newest_pick_and_refuses_when_only_api_picks_remain() {
        let mut loaded = league();
        loaded.api_picks = vec![api_pick(1, "a")];
        apply_manual_pick(&mut loaded, "b".into()).unwrap();
        let removed = undo_manual_pick(&mut loaded).unwrap();
        assert_eq!(removed.player_id, "b");
        assert!(loaded.manual_picks.is_empty());
        let err = undo_manual_pick(&mut loaded).unwrap_err();
        assert!(err.contains("API picks cannot be undone"), "{err}");
        // The API pick was never touched.
        assert_eq!(loaded.api_picks.len(), 1);
    }
}
