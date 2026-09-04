//! The board's own market arithmetic: which ADP column a league drafts on,
//! and what happens to a player who is missing from it.

use super::*;
use crate::sleeper::ProjectionRow;

fn row(id: &str, pairs: &[(&str, f64)]) -> ProjectionRow {
    ProjectionRow {
        player_id: id.into(),
        stats: Some(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), *v))
                .collect::<HashMap<String, f64>>(),
        ),
        player: None,
        week: None,
        opponent: None,
    }
}

/// Forty players whose superflex ADP is consistently 0.8x their PPR ADP —
/// the shape of a real 2QB board, where quarterbacks pull everyone else back.
fn two_qb_rows() -> Vec<ProjectionRow> {
    (1..=40)
        .map(|i| {
            let ppr = i as f64 * 4.0;
            row(
                &format!("p{i}"),
                &[("adp_2qb", ppr * 0.8), ("adp_ppr", ppr)],
            )
        })
        .collect()
}

#[test]
fn a_ppr_league_never_rescales_its_own_column() {
    assert_eq!(adp_fallback_scale(&two_qb_rows(), "adp_ppr"), 1.0);
}

#[test]
fn the_fallback_is_put_on_the_leagues_own_scale() {
    // The old per-row fallback handed this player his raw PPR ADP of 100 on a
    // board where every other number is a 2QB one — twenty-five picks of pure
    // scale error, which reads downstream as a falling-value bargain.
    let mut rows = two_qb_rows();
    rows.push(row("lonely", &[("adp_ppr", 100.0)]));
    let scale = adp_fallback_scale(&rows, "adp_2qb");
    assert!((scale - 0.8).abs() < 1e-9, "median ratio was {scale}");
    assert!((100.0 * scale - 80.0).abs() < 1e-9);
}

#[test]
fn too_few_overlapping_rows_to_measure_leaves_the_scale_alone() {
    // Three rows is not a market. Guessing a ratio off them would be worse
    // than the identity it falls back to.
    let rows: Vec<ProjectionRow> = (1..=3)
        .map(|i| row(&format!("p{i}"), &[("adp_2qb", 1.0), ("adp_ppr", 90.0)]))
        .collect();
    assert_eq!(adp_fallback_scale(&rows, "adp_2qb"), 1.0);
}

#[test]
fn junk_adp_values_are_not_ratio_material() {
    // Sleeper writes 0 and 999 for "no ADP"; either one in the numerator or
    // the denominator would move the median a long way.
    let mut rows = two_qb_rows();
    rows.extend(
        (1..=10).map(|i| row(&format!("junk{i}"), &[("adp_2qb", 0.0), ("adp_ppr", 999.0)])),
    );
    let scale = adp_fallback_scale(&rows, "adp_2qb");
    assert!((scale - 0.8).abs() < 1e-9, "median ratio was {scale}");
}
