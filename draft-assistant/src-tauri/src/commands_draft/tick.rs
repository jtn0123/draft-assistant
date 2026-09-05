//! The pieces of the draft screen that are not themselves commands: one poll
//! tick's fetch, the disk writes the commands and the poll loop do without
//! holding the `loaded` lock, and how long to wait before trying again.
//!
//! Split out of `commands_draft` so that file stays a list of commands.

use super::*;
use crate::traded_picks::{self, TradedPick};

/// What a tick says when Sleeper hands back an empty pick list for a draft
/// that already has picks on it. `/picks` answers `null` now and then, which
/// parses as "no picks"; mid-draft that is a lost response rather than every
/// pick being taken back, so the board is kept and the tick counts as failed.
pub(super) const EMPTY_PICKS: &str =
    "the pick list came back empty — keeping the picks already on the board";

/// The message for a refreshed draft that cannot be laid out, or `None` when
/// it can be.
///
/// Sleeper serves zero teams and zero rounds for a draft that is still being
/// set up. Every board calculation divides by them, so such a draft is not
/// adopted over one that already works.
pub(super) fn unusable(draft: &Draft) -> Option<String> {
    let settings = &draft.settings;
    (settings.teams == 0 || settings.rounds == 0).then(|| {
        format!(
            "the draft came back with {} teams and {} rounds — keeping the ones already on screen",
            settings.teams, settings.rounds
        )
    })
}

/// What one tick should do with the `/draft` resource it asked for beside the
/// picks.
pub(super) enum DraftUpdate {
    /// Adopt this draft: it refreshed and it can be laid out.
    Adopt(Box<Draft>),
    /// The call itself failed. Keep the draft already on screen and say why in
    /// the log, and nowhere else.
    Logged(String),
    /// It answered, with a draft nothing can be laid out from. That is a real
    /// problem with the league on screen rather than a flaky endpoint, so the
    /// user is told.
    Refused(String),
    /// Nothing was asked for. Yahoo has no draft resource to refresh.
    Nothing,
}

/// Read the `/draft` half of a tick.
///
/// A failed call here is deliberately not the picks' problem. Both calls used
/// to drop their errors into one list, and a single failing `/draft` —
/// Sleeper 500s on that endpoint alone often enough — marked the whole tick
/// failed: the sync badge went stale and the backoff stretched the poll to 24
/// seconds while picks were arriving perfectly well every three. A draft
/// resource that does not answer costs nothing, because the status, the order
/// and the timer were read at load and have not changed.
///
/// A draft that *does* answer and cannot be laid out is a different matter:
/// something is wrong with the league on screen, and that is worth saying out
/// loud.
pub(super) fn draft_update(draft: Option<Result<Draft, String>>) -> DraftUpdate {
    match draft {
        None => DraftUpdate::Nothing,
        Some(Err(error)) => DraftUpdate::Logged(format!(
            "draft status not refreshed, keeping the last one: {error}"
        )),
        Some(Ok(draft)) => match unusable(&draft) {
            Some(reason) => DraftUpdate::Refused(reason),
            None => DraftUpdate::Adopt(Box::new(draft)),
        },
    }
}

/// Everything one tick asked the platform for.
pub(super) struct TickFetch {
    /// The pick list. The only half that decides whether a tick failed.
    pub picks: Result<Vec<Pick>, String>,
    /// The draft resource, where the platform has one.
    pub draft: Option<Result<Draft, String>>,
    /// The trade list, where the platform has one.
    pub traded: Option<Result<Vec<TradedPick>, String>>,
}

/// What one tick should do with the `/traded_picks` list it asked for.
///
/// Trades are agreed during a draft, not only before it. The list used to be
/// read once at load, so a pick traded at 8:40pm still drew the old owner on
/// the clock and still counted towards the old owner's "my next picks" for
/// the rest of the night — the one moment in the season when getting that
/// wrong costs a pick.
///
/// A `/traded_picks` call that fails is a note, not a failed tick, for the
/// same reason `/draft` is: the list already on screen stays right, and one
/// sulking endpoint must not stretch the poll or grey the sync badge.
pub(super) fn traded_update(
    traded: Option<Result<Vec<TradedPick>, String>>,
) -> Result<Option<Vec<TradedPick>>, String> {
    match traded {
        None => Ok(None),
        Some(Ok(traded)) => Ok(Some(traded)),
        Some(Err(error)) => Err(format!(
            "traded picks not refreshed, keeping the last list: {error}"
        )),
    }
}

/// Adopt a freshly fetched trade list, and say whether it changed who picks
/// where. Only a changed ownership map is worth redrawing the board for: the
/// array itself comes back in whatever order Sleeper feels like.
pub(super) fn adopt_traded(loaded: &mut LoadedLeague, traded: Vec<TradedPick>) -> bool {
    let season = loaded.draft.season.clone();
    let before = traded_picks::ownership_map(season.as_deref(), &loaded.traded_picks);
    let after = traded_picks::ownership_map(season.as_deref(), &traded);
    loaded.traded_picks = traded;
    before != after
}

/// One refresh of the picks — and, where the platform has them to refresh,
/// the draft resource and the trade list beside them.
///
/// Sleeper serves all three from the draft id. Yahoo has no draft resource at
/// all and no traded-pick list: the picks come from `draftresults` and the
/// team list, and the draft's shape was settled at load time, so there is
/// nothing to re-read and `None` is returned in their place.
pub(super) async fn fetch_tick(
    engine: &Engine,
    yahoo: &YahooState,
    draft_id: &str,
    yahoo_ids: &HashMap<String, String>,
) -> TickFetch {
    if is_yahoo_key(draft_id) {
        return TickFetch {
            picks: yahoo_picks(engine, yahoo, draft_id, yahoo_ids).await,
            draft: None,
            traded: None,
        };
    }
    let (picks, draft, traded) = tokio::join!(
        engine.client.picks(draft_id),
        engine.client.draft(draft_id),
        engine.client.traded_picks(draft_id)
    );
    TickFetch {
        picks: picks.map_err(to_message),
        draft: Some(draft.map_err(|error| error.to_string())),
        traded: Some(traded.map_err(to_message)),
    }
}

/// What one tick needs to know about the league on screen before it goes out.
pub(super) fn tick_target(loaded: &LoadedLeague) -> (String, HashMap<String, String>) {
    (loaded.draft.draft_id.clone(), loaded.yahoo_ids.clone())
}

/// Take one specific pick back out of memory after its write failed, leaving
/// what is on screen matching what is on disk.
///
/// It used to `pop()`. The write happens with the `loaded` lock let go, and a
/// poll tick landing in that window reconciles manual picks the API has
/// caught up with straight out of the list — so the last element was often
/// not the pick that failed. Popping then deleted an unrelated pick the user
/// had typed and left the failed one on the board, which is both halves of
/// the bug at once.
pub(super) async fn undo_pick_in_memory(state: &AppState, draft_id: &str, pick: &Pick) {
    let mut guard = state.loaded.lock().await;
    if let Some(loaded) = guard.as_mut().filter(|l| l.draft.draft_id == draft_id) {
        remove_entered(&mut loaded.manual_picks, pick);
    }
}

/// Take one named pick out of a manual-pick list, and nothing else.
pub(super) fn remove_entered(picks: &mut Vec<Pick>, entered: &Pick) {
    if let Some(at) = picks
        .iter()
        .position(|p| p.pick_no == entered.pick_no && p.player_id == entered.player_id)
    {
        picks.remove(at);
    }
}

/// Build the view for whatever is loaded now. Used by the commands that let
/// go of the lock to write to disk and have to take it again afterwards.
pub(super) async fn view_now(state: &AppState) -> Result<DraftView, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Save a manual-pick list on the blocking pool, with no lock held.
pub(super) async fn save_picks_off_lock(
    engine: &Arc<Engine>,
    draft_id: String,
    picks: Vec<Pick>,
) -> Result<(), String> {
    let engine = engine.clone();
    tokio::task::spawn_blocking(move || engine.save_manual_picks(&draft_id, &picks))
        .await
        .unwrap_or_else(|e| Err(format!("saving your picks failed: {e}")))
}

/// Save a keeper set on the blocking pool, with no lock held. A failure is a
/// warning, not an error: tonight works from the in-memory set either way.
pub(super) async fn save_keepers_off_lock(
    engine: &Arc<Engine>,
    draft_id: String,
    keepers: HashSet<u32>,
) -> Option<String> {
    let engine = engine.clone();
    let written = tokio::task::spawn_blocking(move || engine.save_keepers(&draft_id, &keepers))
        .await
        .unwrap_or_else(|e| Err(format!("keeper save panicked: {e}")));
    written
        .err()
        .map(|error| format!("keepers not saved: {error}"))
}

/// How long to wait before the next tick, given how many ticks in a row have
/// failed.
///
/// A draft that has gone away — the laptop is off wifi, Sleeper is having a
/// bad Sunday — used to be asked again every three seconds forever, which is
/// the worst thing to do to a service that is already struggling and burns
/// battery for nothing. Each consecutive failure doubles the wait, up to
/// eight times the interval and never more than a minute; one success puts it
/// straight back to the interval the user asked for.
pub(crate) fn backoff_secs(interval: u64, failures: u32) -> u64 {
    const MAX_SECS: u64 = 60;
    let factor = 1u64 << failures.min(3);
    interval.saturating_mul(factor).min(MAX_SECS)
}

#[cfg(test)]
mod tests {
    use super::{
        backoff_secs, draft_update, remove_entered, traded_update, DraftUpdate, TradedPick,
    };
    use crate::sleeper::{Draft, DraftSettings, Pick};

    fn draft(teams: u32, rounds: u32) -> Draft {
        Draft {
            draft_id: "D1".into(),
            status: "drafting".into(),
            draft_type: "snake".into(),
            settings: DraftSettings {
                teams,
                rounds,
                ..Default::default()
            },
            draft_order: None,
            start_time: None,
            season: None,
            metadata: None,
            creators: None,
            last_picked: None,
            slot_to_roster_id: None,
        }
    }

    /// `/draft` failing put its error in the same list as the picks', so one
    /// bad endpoint stretched the poll to 24 seconds and greyed the sync badge
    /// while every pick was arriving on time.
    #[test]
    fn a_failed_draft_call_is_a_note_to_log_not_a_failed_tick() {
        let note = match draft_update(Some(Err("Sleeper answered 500".into()))) {
            DraftUpdate::Logged(note) => note,
            _ => panic!("a failed /draft is logged, not counted against the sync"),
        };
        assert!(note.contains("Sleeper answered 500"), "{note}");
        assert!(note.contains("keeping the last one"), "{note}");

        // A draft that answers but cannot be laid out is a different thing:
        // the league on screen has a real problem and the user is told.
        assert!(matches!(
            draft_update(Some(Ok(draft(0, 0)))),
            DraftUpdate::Refused(_)
        ));
        // A good answer is adopted, and Yahoo has nothing to adopt at all.
        assert!(matches!(
            draft_update(Some(Ok(draft(12, 15)))),
            DraftUpdate::Adopt(_)
        ));
        assert!(matches!(draft_update(None), DraftUpdate::Nothing));
    }

    /// A draft that has gone away was asked again every three seconds
    /// forever. Each consecutive failure now doubles the wait, up to eight
    /// times the interval and never past a minute.
    #[test]
    fn consecutive_failures_stretch_the_tick_and_a_success_snaps_it_back() {
        assert_eq!(backoff_secs(3, 0), 3);
        assert_eq!(backoff_secs(3, 1), 6);
        assert_eq!(backoff_secs(3, 2), 12);
        assert_eq!(backoff_secs(3, 3), 24);
        // Capped at eight times the interval, however long the outage runs.
        assert_eq!(backoff_secs(3, 4), 24);
        assert_eq!(backoff_secs(3, 50), 24);
        // And never longer than a minute, whatever the interval.
        assert_eq!(backoff_secs(20, 3), 60);
        assert_eq!(backoff_secs(60, 0), 60);
        assert_eq!(backoff_secs(u64::MAX, 3), 60);
    }

    fn manual(pick_no: u32, player_id: &str) -> Pick {
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

    /// The pick whose write failed used to be taken back with `pop()`. The
    /// write happens with the `loaded` lock let go, so a poll tick landing in
    /// that window can reconcile entries out of the list first — and then the
    /// last element is somebody else's pick. That deleted a pick the user had
    /// typed and left the failed one on the board.
    #[test]
    fn taking_a_failed_pick_back_removes_that_pick_and_not_the_last_one() {
        let failed = manual(12, "rb-1");
        // A reconcile ran while the write was in flight and dropped the
        // failed pick's neighbour, then the user typed another one.
        let mut picks = vec![manual(4, "wr-9"), failed.clone(), manual(15, "te-3")];
        remove_entered(&mut picks, &failed);
        assert_eq!(
            picks.iter().map(|p| p.pick_no).collect::<Vec<_>>(),
            vec![4, 15],
            "the failed pick goes, and only the failed pick"
        );
        // And if the reconcile already removed it, nothing else is taken.
        remove_entered(&mut picks, &failed);
        assert_eq!(picks.len(), 2);
        // Same number, different player, is a different pick.
        remove_entered(&mut picks, &manual(15, "somebody-else"));
        assert_eq!(picks.len(), 2);
    }

    /// A `/traded_picks` that does not answer must not count against the poll
    /// health, for the same reason `/draft` does not: the list already on
    /// screen is still right, and one sulking endpoint would grey the sync
    /// badge and stretch the tick to 24 seconds while picks arrive on time.
    #[test]
    fn a_failed_traded_picks_call_is_a_note_not_a_failed_tick() {
        let note = traded_update(Some(Err("Sleeper answered 500".into())))
            .expect_err("a failed call is reported");
        assert!(note.contains("Sleeper answered 500"), "{note}");
        assert!(note.contains("keeping the last list"), "{note}");
        // Yahoo has no list to refresh at all.
        assert!(matches!(traded_update(None), Ok(None)));
        let fresh = vec![TradedPick {
            season: "2026".into(),
            round: 3,
            roster_id: 10,
            owner_id: 20,
            previous_owner_id: Some(10),
        }];
        assert!(matches!(traded_update(Some(Ok(fresh))), Ok(Some(list)) if list.len() == 1));
    }
}
