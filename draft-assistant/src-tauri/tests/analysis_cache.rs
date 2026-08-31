//! The season poller's hold on the expensive half of a view.
//!
//! Playoff odds, waiver targets and trade ideas cost roughly 1,600 lineup
//! solves plus a playoff simulation plus a trade search, and none of it can
//! change because someone scored a touchdown — so the poller computes them
//! once and hands them back for the next nineteen ticks. Both ends of that
//! window are off-by-one territory: hold one tick too long and a waiver claim
//! is invisible for five minutes, drop one tick too early and the whole point
//! of the cache is gone.

mod common;

use draft_assistant_lib::poll::AnalysisCache;
use draft_assistant_lib::season::{build_season_view_cached, SeasonView};

/// What `commands_season.rs` passes in: rebuild the analysis every 20 ticks.
const ANALYSIS_EVERY: u32 = 20;

/// A real season view over the fixture league, which is what the poller
/// observes — `SeasonAnalysis::of` reads a dozen of its sections.
fn view() -> SeasonView {
    let (loaded, season, config) = common::fixture();
    build_season_view_cached(&loaded, &season, config.my_user_id.as_deref(), None)
}

#[test]
fn there_is_nothing_to_reuse_before_the_first_tick() {
    let cache = AnalysisCache::new(ANALYSIS_EVERY);
    assert!(
        cache.get().is_none(),
        "the first tick has to build the analysis itself"
    );
}

#[test]
fn the_analysis_is_held_for_the_whole_window_and_dropped_at_its_end() {
    let view = view();
    let mut cache = AnalysisCache::new(ANALYSIS_EVERY);

    cache.observe(&view);
    assert!(
        cache.get().is_some(),
        "the first observed view must be kept"
    );

    // Ticks 2..=19 reuse it: eighteen more polls with no rebuild.
    for tick in 2..ANALYSIS_EVERY {
        cache.observe(&view);
        assert!(
            cache.get().is_some(),
            "tick {tick} is inside the window and must still reuse the analysis"
        );
    }

    cache.observe(&view);
    assert!(
        cache.get().is_none(),
        "tick {ANALYSIS_EVERY} closes the window, so the next tick rebuilds"
    );
}

#[test]
fn the_window_starts_again_after_a_rebuild() {
    let view = view();
    let mut cache = AnalysisCache::new(ANALYSIS_EVERY);
    for _ in 0..ANALYSIS_EVERY {
        cache.observe(&view);
    }
    assert!(cache.get().is_none(), "the first window has closed");

    // The tick after a rebuild takes a fresh analysis and holds it for a full
    // window of its own, rather than expiring immediately on the next tick.
    cache.observe(&view);
    assert!(cache.get().is_some(), "the rebuilt analysis is kept");
    for _ in 2..ANALYSIS_EVERY {
        cache.observe(&view);
        assert!(
            cache.get().is_some(),
            "the second window is the same length"
        );
    }
    cache.observe(&view);
    assert!(cache.get().is_none(), "and it closes at the same place");
}

#[test]
fn what_is_held_is_what_a_reusing_tick_reads() {
    // The point of holding it: a view built from the cache reports the age of
    // the ideas it is showing, not the moment it was re-serialised.
    let (loaded, season, config) = common::fixture();
    let my_user_id = config.my_user_id.as_deref();
    let first = build_season_view_cached(&loaded, &season, my_user_id, None);
    let mut cache = AnalysisCache::new(ANALYSIS_EVERY);
    cache.observe(&first);

    let second = build_season_view_cached(&loaded, &season, my_user_id, cache.get());
    assert_eq!(
        second.analysis_as_of_secs, first.analysis_as_of_secs,
        "a reusing tick must not claim the old analysis is new"
    );
    assert_eq!(
        second.standings.len(),
        first.standings.len(),
        "the reused sections must arrive intact"
    );
}

#[test]
fn a_zero_window_is_clamped_rather_than_dividing_by_zero() {
    // `rebuild_every` reaches the cache from a caller-chosen constant, and
    // `ticks % 0` would panic. The clamp turns a meaningless zero into the
    // safest reading of it — rebuild on every tick, cache nothing — which
    // costs time but can never show stale analysis.
    let view = view();
    let mut cache = AnalysisCache::new(0);
    for _ in 0..3 {
        cache.observe(&view);
        assert!(
            cache.get().is_none(),
            "a zero window holds nothing between ticks"
        );
    }
}
