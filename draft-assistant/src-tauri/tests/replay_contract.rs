//! The seam between `bin/dump_state` and `src/replay.ts`.
//!
//! `dump_state` prints `serde_json::to_string_pretty(&build_view(..))` and
//! nothing else; `dump_season` does the same with `build_season_view`. In the
//! browser preview those dumps are all the app gets -- `src/api.ts` builds a
//! `ReplayFeed` over each one, and `src/replay.ts` drives it. Nothing type
//! checks across that line: the Rust side serializes, the TypeScript side
//! reads, and a renamed field is a runtime `undefined` in a browser rather than
//! a compile error anywhere.
//!
//! So this pins the handful of keys the replay path actually touches. It runs
//! the same serialization `dump_state` runs (`build_view`, not the binary --
//! the binary needs the live Sleeper API and a league id), and it reads the
//! TypeScript back off disk to check both halves still agree.
//!
//! Deliberately narrow. `fixture_shape.rs` already guards the *whole* view
//! against the checked-in fixtures; the point here is the much smaller set of
//! fields that `replay.ts` dereferences by name, where a break is silent
//! instead of loud.

mod common;

use draft_assistant_lib::season::{build_season_view, SEASON_SCHEMA_VERSION};
use draft_assistant_lib::view::{build_view, DRAFT_SCHEMA_VERSION};
use serde_json::Value;

/// Read one of the frontend sources. The contract has two halves and only one
/// of them is Rust, so the other half is checked as text.
fn frontend_source(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("src")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The literal in `src/api.ts` that `validateDraftView` compares against.
/// Pulled out of the source rather than duplicated here, so this test cannot
/// be the third copy that drifts.
fn ts_schema_constant(name: &str) -> String {
    let source = frontend_source("api.ts");
    let needle = format!("const {name} = \"");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("src/api.ts no longer declares {name}"))
        + needle.len();
    let rest = &source[start..];
    let end = rest.find('"').expect("unterminated schema constant");
    rest[..end].to_string()
}

/// `dump_state`'s exact output, minus the network: the same `build_view` call,
/// over the deterministic four-team league the other tests use.
fn draft_dump() -> Value {
    let (loaded, _season, config) = common::fixture();
    serde_json::to_value(build_view(&loaded, &config)).expect("serialize the draft view")
}

fn season_dump() -> Value {
    let (loaded, season, config) = common::fixture();
    serde_json::to_value(build_season_view(
        &loaded,
        &season,
        config.my_user_id.as_deref(),
    ))
    .expect("serialize the season view")
}

/// `readDump` does `await response.json()` and hands the result straight to
/// `spec.validate`, so a dump that is not a JSON object fails before any field
/// is looked at.
#[test]
fn both_dumps_are_json_objects_at_the_top_level() {
    assert!(
        draft_dump().is_object(),
        "readDump treats the body as an object"
    );
    assert!(season_dump().is_object());
}

/// `ReplayFeed.poll` reads `spec.generatedAt(next)`, which `api.ts` defines as
/// `view.generated_at`, and compares it with `<=`. A missing key would come
/// back `undefined`, every comparison against it is false, and the feed would
/// push *every* poll through as if it were newer -- the exact bug the
/// "an older dump is ignored" browser test exists to catch.
#[test]
fn every_dump_carries_the_numeric_generated_at_the_replay_feed_orders_by() {
    for (what, dump) in [("draft", draft_dump()), ("season", season_dump())] {
        let at = dump
            .get("generated_at")
            .unwrap_or_else(|| panic!("{what} dump has no generated_at for replay.ts to order by"));
        assert!(
            at.is_number(),
            "{what}.generated_at must be a number, got {at}"
        );
        // `Number.NEGATIVE_INFINITY` is the feed's starting value, so any
        // finite number sorts above it -- but a zero or a negative stamp would
        // mean the clock never ran, and every dump would tie.
        assert!(
            at.as_f64().is_some_and(|n| n > 0.0),
            "{what}.generated_at must be a real instant, got {at}"
        );
    }
}

/// `validateDraftView` / `validateSeasonView` reject the dump outright unless
/// `schema_version` equals a string literal in `api.ts`. Three copies of that
/// version exist -- the Rust constant, the serialized field, and the
/// TypeScript literal -- and the preview breaks the moment any two disagree.
#[test]
fn the_schema_version_agrees_across_rust_the_dump_and_the_typescript_that_validates_it() {
    for (what, dump, rust_constant, ts_name) in [
        (
            "draft",
            draft_dump(),
            DRAFT_SCHEMA_VERSION,
            "DRAFT_VIEW_SCHEMA_VERSION",
        ),
        (
            "season",
            season_dump(),
            SEASON_SCHEMA_VERSION,
            "SEASON_VIEW_SCHEMA_VERSION",
        ),
    ] {
        let serialized = dump
            .get("schema_version")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{what} dump has no schema_version string"));
        assert_eq!(
            serialized, rust_constant,
            "the {what} view serialized a schema_version its own constant does not match"
        );
        assert_eq!(
            serialized,
            ts_schema_constant(ts_name),
            "src/api.ts pins {ts_name} at a different version than the {what} dump carries; \
             bump both together or the browser preview refuses every dump"
        );
    }
}

/// A guard on the guard. If `replay.ts` starts reading a field of the dump
/// directly -- rather than through the `generatedAt` and `validate` callbacks
/// `api.ts` supplies -- this test is no longer pinning the whole contract, and
/// should be extended rather than quietly going stale.
#[test]
fn replay_ts_still_reaches_into_the_dump_only_through_the_feed_spec() {
    let source = frontend_source("replay.ts");
    for accessor in ["spec.generatedAt(", "spec.validate("] {
        assert!(
            source.contains(accessor),
            "replay.ts no longer calls {accessor} -- the dump contract moved, so this test needs \
             to move with it"
        );
    }
    // Anything replay.ts names itself would be a key this file does not check.
    for direct in ["view.generated_at", ".schema_version", "value.draft"] {
        assert!(
            !source.contains(direct),
            "replay.ts now reads `{direct}` off the dump directly; add it to the pinned keys above"
        );
    }
}
