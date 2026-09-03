//! Scoring one candidate, in one mode.
//!
//! Every line here adds a number and says why in the same breath, and the
//! reasons carry what they were worth. The panel only has room for two of
//! them, so they come out ordered by size: the reasons that actually decided
//! the pick, not the two that happened to be computed first.

use super::{Mode, RecommendInputs};
use crate::board::AvailablePlayer;
use std::collections::{HashMap, HashSet};

/// A score being built, and what each reason contributed to it.
pub(crate) struct Score {
    pub total: f64,
    reasons: Vec<(String, f64)>,
}

impl Score {
    fn new() -> Self {
        Score {
            total: 0.0,
            reasons: Vec::new(),
        }
    }

    /// The line shown when every candidate was disqualified.
    pub fn fallback() -> Self {
        Score {
            total: 0.0,
            reasons: vec![("best available (fallback)".into(), 0.0)],
        }
    }

    fn add(&mut self, delta: f64, reason: impl Into<String>) {
        self.total += delta;
        self.reasons.push((reason.into(), delta));
    }

    /// A move the user does not need explained — it still counts.
    fn add_silent(&mut self, delta: f64) {
        self.total += delta;
    }

    /// Biggest mover first. The VORP line leads only when it is the biggest
    /// mover, which on a late-round pick between two negative-VORP bodies it
    /// is not — there the depth and the bye are the whole story.
    pub fn into_reasons(mut self) -> Vec<String> {
        self.reasons.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        self.reasons.into_iter().map(|(text, _)| text).collect()
    }
}

/// Everything the scorer reads, worked out once for all candidates.
pub(crate) struct Context<'a> {
    pub inputs: &'a RecommendInputs<'a>,
    pub open: HashMap<String, u32>,
    pub have: HashMap<&'a str, u32>,
    pub rb_teams: HashSet<&'a str>,
    pub need_pressure: f64,
    pub rounds_left: u32,
    /// Median weekly spread across the board, when there is one to take.
    /// Upside is judged against it rather than against a fixed number: what
    /// counts as a swingy player depends entirely on how variable the source's
    /// weekly projections are at all, and Sleeper's are far flatter than a
    /// real week-to-week distribution — measured against a fixed 0.5 the whole
    /// board reads as identically steady and the signal disappears.
    pub median_cv: Option<f64>,
}

/// `None` when the candidate is disqualified outright at this roster.
pub(crate) fn score_candidate(ctx: &Context, a: &AvailablePlayer, mode: Mode) -> Option<Score> {
    let inputs = ctx.inputs;
    let p = &a.player;
    let mut score = Score::new();
    // Fixed scale: 0.6 pts of score per VORP point. Normalizing by the
    // board-best VORP explodes late in drafts when that value is small.
    score.add(
        p.vorp * 0.6,
        format!("{:.0} VORP under league scoring", p.vorp),
    );

    // How many at this position are left in his tier — thin tiers drive both
    // the scarcity bonus and whether a run on the position is worth chasing.
    let tier_left = inputs
        .available
        .iter()
        .filter(|x| x.player.position == p.position && x.player.tier == p.tier)
        .count();

    need(ctx, a, mode, &mut score);
    discipline(ctx, a, &mut score)?;
    scarcity(a, tier_left, &mut score);
    market(ctx, a, mode, &mut score);
    strategy(ctx, a, tier_left, &mut score);
    injury(ctx, a, mode, &mut score);

    if mode == Mode::Upside {
        upside(ctx, a, &mut score);
    }
    Some(score)
}

/// Roster need: dedicated slot open, then flex eligibility, then depth.
fn need(ctx: &Context, a: &AvailablePlayer, mode: Mode, score: &mut Score) {
    let p = &a.player;
    let open_slot = ctx.inputs.rules.first_open_slot_for(&ctx.open, &p.position);
    if open_slot == Some(p.position.as_str()) {
        score.add(
            12.0 * ctx.need_pressure.min(2.0),
            format!("fills open {} starter slot", p.position),
        );
        return;
    }
    if let Some(slot) = open_slot {
        score.add(
            8.0 * ctx.need_pressure.min(2.0),
            format!("fills an open {slot} slot"),
        );
        return;
    }
    // Depth. The first spare at a position is cheap insurance and the fourth
    // is a wasted roster spot, so the penalty scales with how many I already
    // hold rather than sitting at a flat -10 — which in rounds ten to twelve
    // put every candidate the same distance under water and left the pick to
    // whatever noise was left in the VORP.
    let count = ctx.have.get(p.position.as_str()).copied().unwrap_or(0);
    let beyond = count.saturating_sub(1);
    let mut penalty = (3.0 + 6.0 * beyond as f64).min(24.0);
    // Upside is allowed to buy a lottery ticket at the end of a draft, where
    // the alternative is a body who will never be started either.
    if mode == Mode::Upside && ctx.rounds_left <= 5 {
        penalty *= 0.4;
    }
    score.add(
        -penalty,
        format!("depth pick — {count} {} already rostered", p.position),
    );
}

/// Positional discipline (fantasy-bot's documented failure modes, fixed):
/// backups at onesie positions are near-worthless, and a second DEF is
/// worthless outright. `None` disqualifies the candidate.
fn discipline(ctx: &Context, a: &AvailablePlayer, score: &mut Score) -> Option<()> {
    let p = &a.player;
    let count = ctx.have.get(p.position.as_str()).copied().unwrap_or(0);
    match p.position.as_str() {
        "DEF" | "K" => {
            if count >= 1 {
                return None; // never draft a second defense or kicker
            }
            // Judged on rounds remaining, not on an absolute round index: in
            // a short draft or a mock, "two from the end" is a different
            // round every time and an absolute index reads it wrong.
            if ctx.rounds_left > 3 {
                score.add_silent(-60.0); // never early either
            } else {
                score.add(
                    15.0,
                    format!("last rounds — lock in your one {}", p.position),
                );
            }
        }
        "QB" => {
            if count >= 2 {
                return None;
            }
            if count == 1 {
                score.add(-25.0, "backup QB — only at extreme value");
            }
        }
        "TE" => {
            if count >= 2 {
                return None; // a third TE is a wasted roster spot
            }
            if count == 1 {
                score.add(-20.0, "backup TE — only at real value");
            }
        }
        _ => {
            // RB/WR: reward filling the thin side of the flex pool, and
            // dampen piling past 5 of a kind. Past mid-draft, a position with
            // <2 bodies is one injury from an empty starting slot.
            if count < 2 && ctx.inputs.current_round > 8 {
                score.add(
                    20.0,
                    format!(
                        "only {count} {} rostered — one injury from an empty slot",
                        p.position
                    ),
                );
            }
            if count < 3 {
                score.add(
                    3.0 * (3 - count) as f64,
                    format!("thin at {} ({count} rostered)", p.position),
                );
            } else if count > 5 {
                score.add(
                    -6.0 * (count - 5) as f64,
                    format!("already {count} {}s rostered", p.position),
                );
            }
        }
    }
    Some(())
}

/// Last-of-tier, and whether waiting is an option.
fn scarcity(a: &AvailablePlayer, tier_left: usize, score: &mut Score) {
    let p = &a.player;
    if tier_left <= 2 {
        score.add(
            8.0,
            format!("only {tier_left} left in {} tier {}", p.position, p.tier),
        );
    }
    // Survival: if they'll likely make it back to my next pick, waiting is an
    // option — that lowers urgency now.
    if let Some(surv) = a.survival_next {
        if surv > 0.7 {
            score.add(
                -6.0,
                format!("{:.0}% likely to survive to your next pick", surv * 100.0),
            );
        } else if surv < 0.35 {
            score.add(
                6.0,
                format!(
                    "only {:.0}% chance they last to your next pick",
                    surv * 100.0
                ),
            );
        }
    }
}

/// The market: ADP, the safe mode's reach discipline, and any second opinion.
fn market(ctx: &Context, a: &AvailablePlayer, mode: Mode, score: &mut Score) {
    let p = &a.player;
    // Classic definition: still available past their ADP = falling value;
    // drafting far ahead of ADP = a reach the market says can probably wait.
    if let Some(adp) = p.adp {
        let past_adp = ctx.inputs.current_pick as f64 - adp;
        if past_adp > 8.0 {
            score.add(
                5.0,
                format!("falling: {past_adp:.0} picks past ADP {adp:.0}"),
            );
        } else if past_adp < -25.0 {
            score.add(-3.0, format!("ahead of market (ADP {adp:.0})"));
        }
    }

    // An imported second opinion, in whichever direction it runs. Built next
    // to the rest of the reasons, in `second_opinion.rs`.
    if let Some((delta, reason)) = crate::second_opinion::rec_adjustment(p, ctx.inputs.teams) {
        score.add(delta, reason);
    }

    if mode == Mode::Safe {
        // Volatile bonus-heavy value is not what safe mode is buying.
        score.add_silent(-(p.bonus_points / p.points.max(1.0)) * 40.0);
        if let Some(adp) = p.adp {
            // Stay close to market: the reach is how far ahead of his own ADP
            // this pick is, measured at the pick actually being made. The old
            // form compared his ADP with his rank on *this* board, which is a
            // measure of the two boards disagreeing and was signed so that it
            // penalised bargains and never once penalised a reach.
            let reach = (adp - ctx.inputs.current_pick as f64).max(0.0);
            if reach > 0.0 {
                score.add(
                    -reach * 0.3,
                    format!("a reach: {reach:.0} picks ahead of ADP {adp:.0}"),
                );
            }
        }
    }
}

/// The strategy layer: runs, byes and handcuffs. Small numbers on purpose —
/// these are tie-breakers between players the rest of the score likes equally,
/// not reasons to take somebody the board does not rate.
fn strategy(ctx: &Context, a: &AvailablePlayer, tier_left: usize, score: &mut Score) {
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

/// What a tag is worth off a player's score. Sleeper's codes, worst first.
fn injury_severity(status: &str) -> f64 {
    match status.trim().to_ascii_uppercase().as_str() {
        "" => 0.0,
        "OUT" | "IR" | "PUP" | "SUS" | "NA" | "DNR" | "COV" => 25.0,
        "DOUBTFUL" => 12.0,
        "QUESTIONABLE" => 2.0,
        // An unfamiliar tag is still a tag, and Sleeper adds them.
        _ => 6.0,
    }
}

/// Injuries, scaled by what the tag actually means.
///
/// Both modes read them: balanced ignoring them entirely put men who will not
/// play on the card, and safe docking a flat 15 for any tag at all demoted
/// three of the top five over practice-report "Questionable" — a tag that in
/// August is not about this season at all, which is why it is dropped outright
/// before the draft starts.
fn injury(ctx: &Context, a: &AvailablePlayer, mode: Mode, score: &mut Score) {
    let Some(status) = a.player.injury_status.as_deref() else {
        return;
    };
    let mut severity = injury_severity(status);
    if severity <= 0.0 {
        return;
    }
    if ctx.inputs.pre_draft && status.trim().eq_ignore_ascii_case("questionable") {
        return;
    }
    if mode == Mode::Safe {
        severity *= 1.5;
    }
    score.add(-severity, format!("injury flag: {status}"));
}

/// Upside: pay for the players whose ceiling is real.
///
/// Week-to-week variance is the honest measure, and it comes off the same
/// weekly projections the board already downloads for yardage bonuses. Where
/// there are not enough weeks to measure, the market disagreement stands in:
/// a player this board ranks well ahead of his ADP is one whose value is not
/// yet priced, which is the same bet in a different currency.
fn upside(ctx: &Context, a: &AvailablePlayer, score: &mut Score) {
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
        let disagreement = adp - rank as f64;
        let delta = (disagreement * 0.12).clamp(-4.0, 8.0);
        if delta >= 1.0 {
            score.add(
                delta,
                format!("board has him {disagreement:.0} spots ahead of the market"),
            );
        }
    }
    if p.tier <= 2 && ctx.rounds_left <= 8 {
        score.add(3.0, format!("still a tier {} body this late", p.tier));
    }
}
