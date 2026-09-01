//! Deterministic draft simulation shared by the CLI and regression tests.

use crate::draft::{slot_for_pick, DraftOrder};
use crate::engine::{AppConfig, LoadedLeague};
use crate::sleeper::Pick;
use crate::view::build_view;

/// Apply one simulated pick. The user's slot follows the balanced
/// recommendation; every other slot follows the best remaining ADP.
pub fn apply_simulated_pick(
    loaded: &mut LoadedLeague,
    config: &AppConfig,
    pick_no: u32,
) -> Option<String> {
    let teams = loaded.draft.settings.teams;
    let view = build_view(loaded, config);
    let (order, _) = DraftOrder::from_draft(&loaded.draft);
    let slot = slot_for_pick(pick_no, teams, order);
    let player_id = if view.draft.my_slot == slot {
        view.recommendations
            .iter()
            .find(|recommendation| recommendation.mode == "balanced")
            .map(|recommendation| recommendation.player_id.clone())
    } else {
        view.available
            .iter()
            .filter(|player| player.player.adp.is_some())
            .min_by(|a, b| {
                a.player
                    .adp
                    .partial_cmp(&b.player.adp)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|player| player.player.player_id.clone())
    }?;

    loaded.manual_picks.push(Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: slot?,
        player_id: player_id.clone(),
        picked_by: None,
        metadata: None,
        is_keeper: None,
    });
    Some(player_id)
}
