//! Drift guard for the checked-in browser-preview fixtures.
//!
//! `public/dev-fixture.json` and `public/dev-season-fixture.json` are what the
//! Playwright browser suite renders. They are dumps of a real 14-team league,
//! so they cannot be regenerated here — there is no network in the test
//! environment, and the deterministic `common::fixture()` league is four teams
//! of made-up players. What *can* be checked, and is what actually rots, is
//! the **shape**: every field the current structs serialize must be present in
//! the fixture, and the fixture must not carry fields the structs no longer
//! emit.
//!
//! The comparison is a set of dotted paths (`draft.clock_deadline_ms`,
//! `available[].adp`), with arrays contributing the union of their elements'
//! paths. Neither `DraftView` nor `SeasonView` uses `skip_serializing_if`, so
//! a `None` still shows up as a `null` key and the path set is total: adding
//! an optional field to a struct without touching the fixture fails here.
//!
//! Why paths and not a `deny_unknown_fields` mirror struct: the views are
//! `Serialize`-only, so a mirror would be a second hand-maintained copy of
//! every struct in `view.rs` and `season.rs` — itself a thing that drifts, and
//! one that only catches *extra* fixture keys, never a newly added struct
//! field that the fixture is missing. That missing-field direction is the one
//! that has actually broken, so it is the one pinned hardest here.

mod common;

use draft_assistant_lib::season::{build_season_view, SEASON_SCHEMA_VERSION};
use draft_assistant_lib::simulation::apply_simulated_pick;
use draft_assistant_lib::view::{build_view, DRAFT_SCHEMA_VERSION};
use serde_json::Value;
use std::collections::BTreeSet;

/// Picks simulated before the draft view is built. Enough to fill
/// `recent_picks`, `pick_prices`, and every roster, so no array is empty and
/// the path set is complete on both sides of the comparison.
const SIMULATED_PICKS: u32 = 12;

fn repo_file(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("public")
        .join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The `HashMap` fields of `DraftView`/`SeasonView`: objects whose keys are
/// data (stat names, positions, roster ids) rather than a schema. Listed by
/// hand because nothing in the JSON distinguishes them from a struct — a
/// scoring map and a struct of floats look identical — and because the list is
/// short and a new map field showing up as a diff here is the correct outcome.
const DATA_MAPS: &[&str] = &[
    "league.scoring_settings",
    "replacement_baselines",
    "replacement_demand",
    "draft.pick_slot_overrides",
    "team_avatars",
];

/// Every dotted path in `value`. Arrays collapse to `name[]` and contribute
/// the union of their elements, so a heterogeneous list is covered by one
/// prefix. `DATA_MAPS` collapse to `name{}` for the same reason: their keys
/// carry no schema.
fn paths(value: &Value, prefix: &str, out: &mut Shape) {
    match value {
        Value::Object(map) => {
            if DATA_MAPS.contains(&prefix) {
                out.keys.insert(format!("{prefix}{{}}"));
                for child in map.values() {
                    paths(child, &format!("{prefix}{{}}"), out);
                }
                return;
            }
            for (key, child) in map {
                let next = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                out.keys.insert(next.clone());
                paths(child, &next, out);
            }
        }
        Value::Array(items) => {
            let next = format!("{prefix}[]");
            if items.is_empty() {
                out.empty_arrays.insert(next);
                return;
            }
            for item in items {
                paths(item, &next, out);
            }
        }
        _ => {}
    }
}

/// The dotted key paths of one document, plus the arrays that were empty in
/// it — an empty array says nothing about the shape of what belongs inside.
#[derive(Default)]
struct Shape {
    keys: BTreeSet<String>,
    empty_arrays: BTreeSet<String>,
}

fn shape(value: &Value) -> Shape {
    let mut out = Shape::default();
    paths(value, "", &mut out);
    // A list whose elements are themselves lists — `activity[].players[]` —
    // is empty in some elements and populated in others: the lineup-gap rows
    // name no players, the transaction rows do. That is not a blind spot,
    // because the same document already pins what belongs underneath. Only a
    // prefix this document populated *nowhere* hides anything.
    let populated: BTreeSet<String> = out
        .empty_arrays
        .iter()
        .filter(|prefix| {
            out.keys
                .iter()
                .any(|key| key.starts_with(&format!("{prefix}.")))
        })
        .cloned()
        .collect();
    out.empty_arrays.retain(|p| !populated.contains(p));
    out
}

fn compare(label: &str, generated: &Value, fixture: &Value, unpopulated: &[&str]) {
    let generated = shape(generated);
    let fixture = shape(fixture);
    // An array left empty on one side hides whatever the other side has under
    // it. Arrays of scalars hide nothing, so only the ones with a populated
    // counterpart count as blind spots — and those are pinned by name, so a
    // section quietly going empty cannot silently switch this test off.
    let hides = |empty: &BTreeSet<String>, other: &Shape| -> Vec<String> {
        empty
            .iter()
            .filter(|prefix| {
                other
                    .keys
                    .iter()
                    .any(|key| key.starts_with(&format!("{prefix}.")))
            })
            .cloned()
            .collect()
    };
    let mut blind = hides(&generated.empty_arrays, &fixture);
    blind.extend(hides(&fixture.empty_arrays, &generated));
    blind.sort();
    blind.dedup();
    assert_eq!(
        blind.iter().map(String::as_str).collect::<Vec<_>>(),
        unpopulated,
        "{label}: the sections this test cannot check have changed. Add to the \
         list only with a reason — and prefer making the fixture league \
         produce the section instead."
    );
    let skipped: Vec<String> = blind.iter().map(|prefix| format!("{prefix}.")).collect();
    let known = |path: &&String| !skipped.iter().any(|prefix| path.starts_with(prefix));
    let want: BTreeSet<&String> = generated.keys.iter().filter(known).collect();
    let have: BTreeSet<&String> = fixture.keys.iter().filter(known).collect();
    let mut report = String::new();
    let mut section = |title: &str, items: Vec<&&String>| {
        if items.is_empty() {
            return;
        }
        report.push_str(&format!("\n{title}\n"));
        for item in items {
            report.push_str(&format!("  {item}\n"));
        }
    };
    section(
        "fields the code serializes that the fixture is missing \
         (regenerate the fixture and bump the schema version):",
        want.difference(&have).collect(),
    );
    section(
        "fields in the fixture that the code no longer serializes \
         (drop them from the fixture):",
        have.difference(&want).collect(),
    );
    assert!(report.is_empty(), "public/{label} has drifted:{report}");
}

#[test]
fn draft_fixture_matches_the_current_draft_view() {
    let (mut loaded, _, config) = common::fixture();
    for pick_no in 1..=SIMULATED_PICKS {
        if apply_simulated_pick(&mut loaded, &config, pick_no).is_none() {
            break;
        }
    }
    let view = build_view(&loaded, &config);
    let generated = serde_json::to_value(&view).expect("serialize draft view");
    let fixture = repo_file("dev-fixture.json");
    assert_eq!(
        fixture["schema_version"].as_str(),
        Some(DRAFT_SCHEMA_VERSION),
        "dev-fixture.json is on a different schema version than the code"
    );
    compare("dev-fixture.json", &generated, &fixture, DRAFT_UNPOPULATED);
}

#[test]
fn season_fixture_matches_the_current_season_view() {
    let (loaded, season, config) = common::fixture();
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    let generated = serde_json::to_value(&view).expect("serialize season view");
    let fixture = repo_file("dev-season-fixture.json");
    assert_eq!(
        fixture["schema_version"].as_str(),
        Some(SEASON_SCHEMA_VERSION),
        "dev-season-fixture.json is on a different schema version than the code"
    );
    compare(
        "dev-season-fixture.json",
        &generated,
        &fixture,
        SEASON_UNPOPULATED,
    );
}

/// Arrays the four-team fixture league leaves empty in the draft view.
const DRAFT_UNPOPULATED: &[&str] = &[];

/// Arrays the four-team fixture league leaves empty in the season view: it has
/// no roster with a surplus to offer, so the trade finder proposes nothing.
const SEASON_UNPOPULATED: &[&str] = &["trades[]"];

/// The path comparison above is blind to a *value* that has gone stale: an
/// empty-but-present array contributes no paths, so `recent_trades[].sides[]`
/// kept passing while three of the five deals still carried `"gets": []` —
/// captured before `season_deals::picks_for` learned to name traded picks, and
/// rendering as the uninformative "gets draft picks" fallback in the preview
/// and the whole Playwright suite.
///
/// A general "derived array the code would now populate" check would mean
/// replaying the fixture's own transactions through the view builder, which
/// needs the real league's players and rosters — the very thing this file
/// explains it cannot regenerate. This is the cheap half: every side of a real
/// trade received *something*, so an empty `gets` in the fixture is stale by
/// construction, whatever the reason.
#[test]
fn every_trade_side_in_the_season_fixture_names_what_it_got() {
    let fixture = repo_file("dev-season-fixture.json");
    let deals = fixture["recent_trades"]
        .as_array()
        .expect("recent_trades is an array");
    assert!(!deals.is_empty(), "the fixture has no trades to check");
    for deal in deals {
        for side in deal["sides"].as_array().expect("sides is an array") {
            let gets = side["gets"].as_array().expect("gets is an array");
            assert!(
                !gets.is_empty(),
                "trade {} side {} has an empty `gets`: a side that received nothing is a stale \
                 capture, and prints as the \"draft picks\" fallback",
                deal["transaction_id"],
                side["team"],
            );
        }
    }
}
