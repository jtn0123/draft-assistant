//! What an injury tag costs a candidate.
//!
//! It used to cost a flat 25 points, against a VORP term worth 0.6 of a
//! player's whole value over replacement. A 90-VORP running back on injured
//! reserve therefore came out 54 minus 25, still the best card on the board,
//! and the panel recommended a man who would not play a down. The tag is not
//! a fixed penalty, it is a share of the season: what an injury takes away is
//! the games, and the games are what the VORP was counting.

use super::{Mode, RecommendInputs, Score};
use crate::board::AvailablePlayer;

/// The floor under a season-ending tag, in score points. A replacement-level
/// body on injured reserve is still worse than the same body healthy, and
/// with a share-of-value term alone a zero-VORP player would be docked
/// nothing at all for missing the year.
const MISSING_BODY: f64 = 8.0;

/// What a tag costs: the share of the season it takes away, and the line that
/// says so in the tag's own words.
enum Tag {
    /// A share of the season gone, priced against the player's own value.
    Missing(f64, String),
    /// A weekly practice-report tag. Worth a couple of points, flat: it is
    /// about Sunday, not about the season the VORP measures.
    Weekly(f64),
}

fn classify(status: &str, weeks_left: u32) -> Option<Tag> {
    let code = status.trim().to_ascii_uppercase();
    match code.as_str() {
        "" => None,
        // Out for the year, or as near as makes no difference.
        "IR" | "PUP" | "NA" | "DNR" => Some(Tag::Missing(
            0.9,
            format!("on {status}: most of the season gone"),
        )),
        // "Out" is a ruling for one game. Pricing it at a quarter of the
        // season charged a man eighteen times what his absence costs, and in
        // week fifteen it charged him for weeks that will never be played.
        "OUT" => Some(Tag::Missing(
            1.0 / f64::from(weeks_left.max(1)),
            format!("tagged {status}: a week of the season gone"),
        )),
        // A suspension and a reserve list both have a length, and it is
        // measured in weeks rather than in Sundays.
        "SUS" | "COV" => Some(Tag::Missing(
            0.25,
            format!("tagged {status}: several weeks of the season gone"),
        )),
        "DOUBTFUL" => Some(Tag::Weekly(6.0)),
        "QUESTIONABLE" => Some(Tag::Weekly(2.0)),
        // An unfamiliar tag is still a tag, and Sleeper adds them.
        _ => Some(Tag::Weekly(6.0)),
    }
}

/// A practice-report tag, which before a draft is left over from last season
/// and says nothing about the one being drafted. "Doubtful" is one of these
/// exactly as much as "Questionable" is — dropping only the latter left a
/// stale August "Doubtful" taking nine points off a safe-mode card.
fn is_practice_report(status: &str) -> bool {
    let code = status.trim().to_ascii_uppercase();
    matches!(code.as_str(), "QUESTIONABLE" | "DOUBTFUL")
}

/// Injuries, priced by what the tag actually takes away.
///
/// Both modes read them: balanced ignoring them entirely put men who will not
/// play on the card, and safe docking a flat 15 for any tag at all demoted
/// three of the top five over practice-report "Questionable" — a tag that in
/// August is not about this season at all, which is why it is dropped outright
/// before the draft starts.
pub(crate) fn injury(a: &AvailablePlayer, inputs: &RecommendInputs, mode: Mode, score: &mut Score) {
    let Some(status) = a.player.injury_status.as_deref() else {
        return;
    };
    if inputs.pre_draft && is_practice_report(status) {
        return;
    }
    let Some(tag) = classify(status, inputs.weeks_left) else {
        return;
    };
    // Safe mode buys the games it can count on, so it reads every tag harder.
    let weight = if mode == Mode::Safe { 1.5 } else { 1.0 };
    match tag {
        Tag::Missing(share, reason) => {
            // The same 0.6 a point of VORP is worth to the score above, times
            // the share of the season that will not be played. Negative VORP
            // is not value an injury can take away, so it is floored at zero
            // and only the missing-body term remains.
            let value = 0.6 * a.player.vorp.max(0.0) + MISSING_BODY;
            score.add(-share * value * weight, reason);
        }
        Tag::Weekly(flat) => {
            score.add(-flat * weight, format!("injury flag: {status}"));
        }
    }
}
