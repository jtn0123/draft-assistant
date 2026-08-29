//! Keepers noticed during a draft are remembered across launches, so a
//! keeper stays a keeper once the draft passes its slot.

use crate::engine::Engine;
use crate::loaded::LoadedLeague;
use crate::log;
use crate::view;

/// Fold newly seen keepers into the league's memory of them: judged from
/// where each pick sits now, and never forgotten once judged.
pub(crate) fn note_keepers(engine: &Engine, loaded: &mut LoadedLeague) {
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    let seen = view::keeper_pick_nos(&loaded.api_picks, teams, rounds);
    let before = loaded.keeper_pick_nos.len();
    loaded.keeper_pick_nos.extend(seen);
    if loaded.keeper_pick_nos.len() != before {
        if let Err(error) = engine.save_keepers(&loaded.draft.draft_id, &loaded.keeper_pick_nos) {
            log::warn(format!("keepers not saved: {error}"));
        }
    }
}
