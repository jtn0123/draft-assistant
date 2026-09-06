//! The strategy layer and the upside bet: the terms that break ties between
//! players the rest of the score likes equally, and the one mode that pays
//! for a ceiling. Split from `recommend_score.rs` for length; the scoring
//! order is still set there.

use super::{Context, Score};
use crate::board::AvailablePlayer;

/// The strategy layer: runs, byes and handcuffs. Small numbers on purpose —
/// these are tie-breakers between players the rest of the score likes equally,
/// not reasons to take somebody the board does not rate.
pub(super) fn strategy(ctx: &Context, a: &AvailablePlayer, tier_left: usize, score: &mut Score) {
    let p = &a.player;
    // A run only matters if the tier it is eating is nearly gone. A run on a
    // position with forty bodies left is other people making a mistake.
    if let Some(run) = ctx.inputs.position_run {
        if run.position == p.position && tier_left <= 3 {
            score.add(
                4.0,
                format!(
                    "run on {}: {} of the last {} picks, {tier_left} left in tier {}",
                    run.position, run.count, run.window, p.tier
                ),
            );
        }
    }
    // Byes: a starting lineup with four men off in week 9 loses week 9.
    if let Some(bye) = p.bye_week {
        let stacked = ctx.inputs.my_byes.get(&bye).copied().unwrap_or(0);
        if stacked > 0 {
            let penalty = (3.0 * stacked as f64).min(9.0);
            score.add(
                -penalty,
                format!("week {bye} bye, shared with {stacked} of your starters"),
            );
        }
    }
    // Handcuff: the back behind a back I already own. Approximated by the NFL
    // team, which is all the board knows — a depth chart is not something
    // Sleeper's projections carry.
    if p.position == "RB" {
        if let Some(team) = p.team.as_deref() {
            if ctx.rb_teams.contains(team) {
                score.add(5.0, format!("handcuffs the {team} back you already have"));
            }
        }
    }
}

/// Upside: pay for the players whose ceiling is real.
///
/// Week-to-week variance is the honest measure, and it comes off the same
/// weekly projections the board already downloads for yardage bonuses. Where
/// there are not enough weeks to measure, the market disagreement stands in:
/// a player this board ranks well ahead of his ADP is one whose value is not
/// yet priced, which is the same bet in a different currency.
pub(super) fn upside(ctx: &Context, a: &AvailablePlayer, score: &mut Score) {
    let p = &a.player;
    if let (Some(cv), Some(median)) = (p.weekly_cv, ctx.median_cv) {
        // How much swingier than the middle of this board he is.
        let ratio = cv / median;
        let delta = ((ratio - 1.0) * 4.0).clamp(-6.0, 10.0);
        if delta.abs() >= 0.5 {
            score.add(
                delta,
                format!("week to week he swings {ratio:.1}x what the middle of this board does"),
            );
        }
    }
    if let (Some(adp), rank) = (p.adp, p.overall_rank) {
        // Positive when the market drafts him later than this board ranks him.
        // And negative when the market likes him more than this board does,
        // which is the same bet the other way: his ceiling is already in his
        // price. The negative half used to be clamped to minus four and then
        // gated on `delta >= 1.0`, so it could never fire and upside mode
        // paid the same for a player priced above his ceiling as for one
        // priced below it.
        let disagreement = adp - rank as f64;
        let delta = (disagreement * 0.12).clamp(-4.0, 8.0);
        if delta.abs() >= 1.0 {
            let reason = if delta > 0.0 {
                format!("board has him {disagreement:.0} spots ahead of the market")
            } else {
                format!(
                    "market has him {:.0} spots ahead of this board",
                    -disagreement
                )
            };
            score.add(delta, reason);
        }
    }
    if p.tier <= 2 && ctx.rounds_left <= 8 {
        score.add(3.0, format!("still a tier {} body this late", p.tier));
    }
}
