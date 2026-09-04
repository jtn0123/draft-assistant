//! The pieces of the draft screen that are not themselves commands: one poll
//! tick's fetch, the disk writes the commands and the poll loop do without
//! holding the `loaded` lock, and how long to wait before trying again.
//!
//! Split out of `commands_draft` so that file stays a list of commands.

use super::*;

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

/// One refresh of the picks — and, where the platform has one to refresh, the
/// draft resource beside them.
///
/// Sleeper serves both from the draft id. Yahoo has no draft resource at all:
/// the picks come from `draftresults` and the team list, and the draft's shape
/// was settled at load time, so there is nothing to re-read and `None` is
/// returned in its place.
pub(super) async fn fetch_tick(
    engine: &Engine,
    yahoo: &YahooState,
    draft_id: &str,
    yahoo_ids: &HashMap<String, String>,
) -> (Result<Vec<Pick>, String>, Option<Result<Draft, String>>) {
    if is_yahoo_key(draft_id) {
        return (yahoo_picks(engine, yahoo, draft_id, yahoo_ids).await, None);
    }
    let (picks, draft) = tokio::join!(engine.client.picks(draft_id), engine.client.draft(draft_id));
    (
        picks.map_err(to_message),
        Some(draft.map_err(|error| error.to_string())),
    )
}

/// What one tick needs to know about the league on screen before it goes out.
pub(super) fn tick_target(loaded: &LoadedLeague) -> (String, HashMap<String, String>) {
    (loaded.draft.draft_id.clone(), loaded.yahoo_ids.clone())
}

/// Take the pick back out of memory after its write failed, leaving what is
/// on screen matching what is on disk.
pub(super) async fn undo_pick_in_memory(state: &AppState, draft_id: &str) {
    let mut guard = state.loaded.lock().await;
    if let Some(loaded) = guard.as_mut().filter(|l| l.draft.draft_id == draft_id) {
        loaded.manual_picks.pop();
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
pub(super) fn backoff_secs(interval: u64, failures: u32) -> u64 {
    const MAX_SECS: u64 = 60;
    let factor = 1u64 << failures.min(3);
    interval.saturating_mul(factor).min(MAX_SECS)
}

#[cfg(test)]
mod tests {
    use super::backoff_secs;

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
}
