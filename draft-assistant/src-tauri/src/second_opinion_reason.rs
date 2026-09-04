//! What an imported second opinion is worth on a recommendation card.
//!
//! Split out of `second_opinion.rs`, which is the file that reads the CSV in;
//! this is the one line of the draft score that reads it back out, and the
//! only part of the feature the recommender ever calls.

use super::DISAGREEMENT;
use crate::board::BoardPlayer;

/// What an imported second opinion is worth on the rec card: a score
/// adjustment and the line that explains it.
///
/// Symmetric, because a source that likes a player *less* than this board is
/// exactly as informative as one that likes him more — the old one-directional
/// version quietly bumped every disagreement upward and never once warned the
/// user off. Proportional, because WR9-vs-WR21 and WR9-vs-WR60 are not the
/// same disagreement: a quarter of a point per position of gap, capped either
/// way so an unmatched-looking row cannot swamp the VORP the score is built on.
///
/// `points_per_reception` is what *this* league pays for a catch. The imported
/// ranks are half-PPR, and in a standard or a full-PPR league they are a
/// different sport at the receiving positions — a possession receiver is two
/// rounds apart between the two. Rather than throw the file away, the rank gap
/// is worth half as much there, and the line says which ruler it was measured
/// with so the user can discount it themselves.
pub fn rec_adjustment(
    player: &BoardPlayer,
    teams: u32,
    points_per_reception: f64,
) -> Option<(f64, String)> {
    let opinion = player.second_opinion.as_ref()?;
    let gap = player.position_rank as i64 - opinion.positional_rank as i64;
    if gap.abs() < DISAGREEMENT {
        return None; // a few places apart is noise, in either direction
    }
    let near_half_ppr = (0.25..0.75).contains(&points_per_reception);
    let weight = if near_half_ppr { 1.0 } else { 0.5 };
    let caveat = if near_half_ppr {
        ""
    } else {
        " (half-PPR ranks)"
    };
    let delta = (gap as f64 / 4.0).clamp(-8.0, 8.0) * weight;
    let headline = format!(
        "{} has him {}{}",
        opinion.source, player.position, opinion.positional_rank
    );
    if gap < 0 {
        return Some((
            delta,
            format!(
                "{headline}, well behind this board's {}{}{caveat}",
                player.position, player.position_rank
            ),
        ));
    }
    // How far behind the source's own overall rank the market is still
    // drafting him, in rounds of this league.
    if let Some(adp) = player.adp {
        let rounds = (adp - opinion.overall_rank as f64) / teams.max(1) as f64;
        if rounds >= 1.0 {
            let rounds = rounds.round() as u32;
            let plural = if rounds == 1 { "" } else { "s" };
            return Some((
                delta,
                format!("{headline} — market is {rounds} round{plural} late{caveat}"),
            ));
        }
    }
    Some((
        delta,
        format!(
            "{headline}; this board has him {}{}{caveat}",
            player.position, player.position_rank
        ),
    ))
}
