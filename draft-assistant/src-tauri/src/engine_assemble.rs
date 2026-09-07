//! The half of a league load that has nothing to do with the platform.
//!
//! [`Engine::load_league`] fetches from Sleeper and
//! [`Engine::load_yahoo_league`](crate::engine_yahoo) fetches from Yahoo, but
//! once both have a `League`, a `Draft`, a pick list and a player dictionary
//! the rest is identical: reconcile the hand-typed picks, refuse a draft that
//! has not been set up, score the board, apply the imported second opinion,
//! remember the keepers. That shared tail lives here so neither loader owns
//! it and the two cannot drift apart.

use crate::board::build_board;
use crate::engine::{Engine, LoadedLeague};
use crate::keepers::KeeperStore;
use crate::picks::{reconcile_manual_picks, ManualPickStore};
use crate::roster::RosterRules;
use crate::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow};
use crate::traded_picks::TradedPick;
use crate::weekly::WeeklyPoints;
use std::collections::HashMap;

/// Everything a loader has to hand over. One struct rather than fifteen
/// arguments, and every field is something only the platform side can know.
pub(crate) struct AssemblyParts {
    pub league: League,
    pub draft: Draft,
    /// Whatever names slots by; empty when the platform has no member list.
    pub user_names: HashMap<String, String>,
    pub user_avatars: HashMap<String, String>,
    pub api_picks: Vec<Pick>,
    pub traded_picks: Vec<TradedPick>,
    /// See [`LoadedLeague::my_slot`]: set by a platform that names the
    /// logged-in user's own team, `None` by one that does not.
    pub my_slot: Option<u32>,
    /// See [`LoadedLeague::yahoo_ids`]. Empty from the Sleeper loader.
    pub yahoo_ids: HashMap<String, String>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
    /// When the player dictionary was fetched, and the rows themselves keyed
    /// by the id the picks and the projections use.
    pub players: (u64, HashMap<String, PlayerMeta>),
    pub season_projections: (u64, Vec<ProjectionRow>),
    pub weekly: (u64, Vec<ProjectionRow>),
    /// Anything the loader wants the user to see, before the board adds its
    /// own.
    pub warnings: Vec<String>,
}

impl Engine {
    /// Turn one loader's parts into the loaded league the whole app reads.
    ///
    /// Synchronous on purpose: every fetch is already done by the time this
    /// is called, and what is left is arithmetic plus three small file reads.
    pub(crate) fn finish_assembly(&self, parts: AssemblyParts) -> Result<LoadedLeague, String> {
        let AssemblyParts {
            league,
            draft,
            user_names,
            user_avatars,
            api_picks,
            traded_picks,
            my_slot,
            yahoo_ids,
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players: (players_fetched_at, player_meta),
            season_projections: (projections_fetched_at, season_rows),
            weekly: (weekly_fetched_at, weekly_rows),
            mut warnings,
        } = parts;

        // The four file touches in this tail, the manual picks, the second
        // opinion, the keepers and the write-back of reconciled picks, step
        // off the async pool: this runs inside the load command, and a slow
        // disk here used to hold the poll tick and every other command for
        // the length of each read.
        let mut manual_picks = off_runtime(|| self.load_manual_picks(&draft.draft_id));
        if reconcile_manual_picks(&api_picks, &mut manual_picks) {
            off_runtime(|| self.save_manual_picks(&draft.draft_id, &manual_picks))?;
        }
        // Every pick calculation divides by the team count and counts up to
        // teams * rounds, so a draft that reports neither is refused here
        // rather than panicking on the next view build.
        if draft.settings.teams == 0 || draft.settings.rounds == 0 {
            return Err(format!(
                "draft {} reports {} teams and {} rounds, it has not been set up yet",
                draft.draft_id, draft.settings.teams, draft.settings.rounds
            ));
        }
        if let Some(error) = &poll_last_error {
            warnings.push(format!("initial picks refresh failed: {error}"));
        }
        let scoring_map = league.scoring_settings.clone();
        let roster_rules = RosterRules::new(&league.roster_positions);
        // Said once, at load: a slot no draftable position fills (an IDP
        // league's DL, LB, DB) gets no players from this board and is left
        // out of the open-starter count, and a user staring at an empty
        // linebacker slot deserves to hear that from the app and not from
        // the board.
        let unfillable = roster_rules.unfillable_slots();
        if !unfillable.is_empty() {
            warnings.push(format!(
                "this board does not draft for {}: those slots are ignored by the advice",
                unfillable.join(", ")
            ));
        }
        let board_build = build_board(
            &league,
            &draft,
            &player_meta,
            &season_rows,
            &weekly_rows,
            &roster_rules,
            &mut warnings,
        );
        let mut board = board_build.players;
        // The imported second opinion, if the user has ever chosen one. A file
        // that has stopped parsing becomes a warning rather than a failed
        // load: it is a nice-to-have column, not the board.
        let second_opinion_loaded_at =
            match off_runtime(|| crate::second_opinion::load(&self.data_dir)) {
                Ok(Some(table)) => {
                    let report = crate::second_opinion::apply(&table, &mut board);
                    if report.matched == 0 {
                        warnings.push(
                            "imported projections matched nobody on this board, \
                         check it is the right season"
                                .into(),
                        );
                    }
                    Some(table.loaded_at)
                }
                Ok(None) => None,
                Err(error) => {
                    warnings.push(format!("imported projections could not be read: {error}"));
                    None
                }
            };
        if board.len() < 200 {
            warnings.push(format!(
                "board unusually small ({} players), projections may be incomplete",
                board.len()
            ));
        }
        let board_index: HashMap<String, usize> = board
            .iter()
            .enumerate()
            .map(|(i, p)| (p.player_id.clone(), i))
            .collect();

        // Anything that failed to reach the disk while this league was being
        // loaded — a full disk, a data directory that has gone read-only —
        // goes on screen. It used to be discarded, so the only symptom was a
        // cold, slow start every single launch with nothing said.
        warnings.extend(self.take_cache_warnings());
        // Where the clock stands right now is the last gap in this draft's
        // pick list that can be trusted to mean "keeper". Every later snapshot
        // is judged against it, so one `/picks` answer that drops a pick
        // cannot brand the rest of the board. See `keepers::evidence`.
        let remembered = off_runtime(|| self.load_keepers(&draft.draft_id));
        let floor =
            crate::picks::next_open_pick(&api_picks, draft.settings.teams, draft.settings.rounds);
        // A keeper already passed cannot be told from a pick the room made,
        // so a first load of a draft in progress knows less about this league
        // than it looks like it does. See `keepers::unseen_keeper_warning`.
        warnings.extend(crate::keepers::unseen_keeper_warning(
            &api_picks,
            &remembered,
            floor,
        ));
        let keeper_pick_nos = crate::keepers::KeeperMemory {
            picks: remembered,
            floor,
        };
        Ok(LoadedLeague {
            league,
            draft,
            user_names,
            user_avatars,
            my_slot,
            yahoo_ids,
            // Behind `Arc` because the season poll tick copies the whole
            // loaded league out from under its mutex every thirty seconds:
            // a deep copy of the board, its index, the dictionary and the
            // weekly projections was megabytes of map cloning per tick.
            board: std::sync::Arc::new(board),
            board_index: std::sync::Arc::new(board_index),
            replacement_model: board_build.replacement,
            roster_rules,
            api_picks,
            manual_picks,
            traded_picks,
            keeper_pick_nos,
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players_fetched_at,
            projections_fetched_at,
            weekly_fetched_at,
            warnings,
            weekly_points: std::sync::Arc::new(WeeklyPoints::build_with(
                &weekly_rows,
                &scoring_map,
                &player_meta,
            )),
            player_meta: std::sync::Arc::new(player_meta),
            second_opinion_loaded_at,
        })
    }
}

/// Run a small piece of disk work without stalling the async runtime.
///
/// `finish_assembly` is synchronous and called from inside an async load, so
/// its file reads cannot be awaited; `block_in_place` moves the calling
/// thread out of the async pool for the duration instead, the same way
/// `cache::envelope_json_off_runtime` does for the big encode. A
/// current-thread runtime has no pool and `block_in_place` panics there, so
/// tests and the `dump_*` binaries run the work inline.
fn off_runtime<T>(work: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current().map(|handle| handle.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}
