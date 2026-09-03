//! The two provenance labels a projections CSV can carry, and what they cost
//! the row that carries them.
//!
//! The script that writes the file knows which of its numbers came from a
//! provider and which it invented; these columns are how it says so, and this
//! module is the only place that reads them. See the note at the top of
//! `second_opinion.rs` for why an invented row cannot simply be ranked with
//! the rest.

use serde::Serialize;

/// Rows the file carried that this module refused to rank, by cause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Excluded {
    /// Rows whose points the script invented from its ADP curve.
    pub adp_estimate: usize,
    /// Rows ranked off a week-one matchup page rather than a season one.
    pub week_1_ranking: usize,
}

impl Excluded {
    pub fn total(&self) -> usize {
        self.adp_estimate + self.week_1_ranking
    }

    /// The half-sentence the toast appends, or `None` when a file was clean
    /// — including every file written before the labels existed.
    pub fn reason(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if self.adp_estimate > 0 {
            parts.push(format!("{} estimated from ADP", self.adp_estimate));
        }
        if self.week_1_ranking > 0 {
            let plural = if self.week_1_ranking == 1 { "" } else { "s" };
            parts.push(format!(
                "{} week-1 defence ranking{plural}",
                self.week_1_ranking
            ));
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join(", "))
    }
}

/// `projection_method` values that mean "this number was invented".
const INVENTED_POINTS: &str = "adp_estimate";
/// The `ranking_basis` prefix that means "this ranking is about week one".
const WEEK_1_BASIS: &str = "week_1_matchup";

/// Why a row cannot be ranked, read off its two provenance labels. `None`
/// when the row is usable — which is every row of a file that has no labels.
pub(super) fn unrankable(method: Option<&str>, basis: Option<&str>) -> Option<Cause> {
    let method = method.unwrap_or_default().trim().to_ascii_lowercase();
    if method == INVENTED_POINTS {
        return Some(Cause::AdpEstimate);
    }
    let basis = basis.unwrap_or_default().trim().to_ascii_lowercase();
    if basis.starts_with(WEEK_1_BASIS) {
        return Some(Cause::Week1Ranking);
    }
    None
}

/// The two ways a row can be unrankable.
#[derive(Debug, Clone, Copy)]
pub(super) enum Cause {
    AdpEstimate,
    Week1Ranking,
}
