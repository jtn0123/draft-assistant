//! Scoring one candidate, in one mode.
//!
//! Every line here adds a number and says why in the same breath, and the
//! reasons carry what they were worth. The panel only has room for two of
//! them, so they come out ordered by size: the reasons that actually decided
//! the pick, not the two that happened to be computed first.

use super::{Mode, RecommendInputs};
use crate::board::AvailablePlayer;
use std::collections::{HashMap, HashSet};

#[path = "recommend_demand.rs"]
mod demand;
#[path = "recommend_injury.rs"]
mod injury;

use demand::dedicated_starters;
pub(crate) use demand::{starters_phrase, starting_demand};

/// A pick-count threshold fitted on a twelve-team league, restated in this
/// league's picks. Rounded, because a threshold measured in picks that lands
/// on a fraction of one is a false precision.
fn league_scaled(ctx: &Context, picks_at_twelve: f64) -> f64 {
    (picks_at_twelve * ctx.inputs.teams.max(1) as f64 / 12.0).round()
}

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

    /// What each reason was worth, for the test that the shown reasons and
    /// the score are the same arithmetic. Nothing may move the total without
    /// saying so: two terms used to (the early K/DEF veto and safe mode's
    /// bonus-dependence discount), and a card whose reasons cannot be added
    /// back up to its score is a card the user cannot audit.
    #[cfg(test)]
    pub(crate) fn weights(&self) -> Vec<f64> {
        self.reasons.iter().map(|(_, delta)| *delta).collect()
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
    /// Per-team starting demand by position, allocated the way the
    /// replacement level is. Worked out once for the whole board.
    pub demand: HashMap<String, f64>,
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
    injury::injury(a, ctx.inputs.pre_draft, mode, &mut score);

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
                score.add(
                    -60.0,
                    format!(
                        "far too early for a {} — {} rounds still to draft",
                        p.position, ctx.rounds_left
                    ),
                );
            } else {
                score.add(
                    15.0,
                    format!("last rounds — lock in your one {}", p.position),
                );
            }
        }
        // The onesie positions — except that whether they are onesies is a
        // property of the league, not of the position. A superflex league
        // starts two quarterbacks and a TE-premium league sometimes starts
        // two tight ends; keying the cap on the number one docked the second
        // quarterback in a superflex draft twenty-five points for being a
        // backup, when he was the single largest hole on the roster, and
        // refused the third outright when he was ordinary depth.
        "QB" | "TE" => {
            let starters = dedicated_starters(ctx.inputs.rules, p.position.as_str()).max(1);
            // One spare beyond what the league starts, and no more.
            if count > starters {
                return None;
            }
            if count == starters {
                let (penalty, reason) = if p.position == "QB" {
                    (-25.0, "backup QB — only at extreme value")
                } else {
                    (-20.0, "backup TE — only at real value")
                };
                score.add(penalty, reason);
            }
        }
        _ => {
            // RB/WR: reward filling the thin side of the flex pool, and
            // dampen piling past 5 of a kind. Past mid-draft, a position with
            // <2 bodies is one injury from an empty starting slot.
            // Only when the need layer above has not already paid for the
            // same hole. Both fired on the same position and the score
            // counted one empty starting slot twice, 20 here on top of
            // need's 12 times the pressure, which is how a fourth receiver
            // outbid a first-round back in round nine.
            let already_paid = ctx
                .inputs
                .rules
                .first_open_slot_for(&ctx.open, &p.position)
                .is_some();
            if count < 2 && ctx.inputs.current_round > 8 && !already_paid {
                score.add(
                    20.0,
                    format!(
                        "only {count} {} rostered — one injury from an empty slot",
                        p.position
                    ),
                );
            }
            if count > 5 {
                score.add(
                    -6.0 * (count - 5) as f64,
                    format!("already {count} {}s rostered", p.position),
                );
            }
        }
    }

    // Early depth, priced off what this league actually starts. The term it
    // replaces was a flat +3 per body short of three, for running backs and
    // receivers only — so a quarterback and a tight end began every draft
    // nine points behind, in a scoring system where nine points is the whole
    // gap between the top two cards, and no part of that head start had
    // anything to do with the league's roster.
    if !crate::board::is_late_only(&p.position) {
        let demand = ctx.demand.get(p.position.as_str()).copied().unwrap_or(0.0);
        let short = (demand - count as f64).max(0.0);
        if short > 0.05 {
            score.add(
                3.0 * short,
                format!(
                    "thin at {}: {count} rostered, the league starts {}",
                    p.position,
                    starters_phrase(ctx.inputs.rules, p.position.as_str(), demand)
                ),
            );
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
    // Measured at the *market* pick, never the board's own pick number. ADP
    // counts selections; a keeper is entered hours before anybody is on the
    // clock and nobody ever selects at its number. With twenty-six keepers in
    // the book, pick 30 is the fourth name called, and reading it as the
    // thirtieth said an ADP-12 player had fallen eighteen picks past his
    // market — a bargain bonus paid on every single card, all night.
    //
    // The thresholds themselves are league-sized: "eight picks past ADP" is
    // two-thirds of a round in a twelve-team league and over four fifths of
    // one in a ten, and the round is the unit a drafter actually feels.
    if let Some(adp) = p.adp {
        let past_adp = ctx.inputs.market_pick as f64 - adp;
        let falling = league_scaled(ctx, 8.0);
        let ahead = league_scaled(ctx, 25.0);
        // Both terms are worth what the distance is worth, capped. Flat
        // numbers here meant a man nine picks past his ADP and a man
        // sixty-two picks past him got the same five points, under a reason
        // line that quoted the real distance — the card said sixty-two and
        // the score said eight. The cap is there because past a few rounds
        // the market has stopped saying "bargain" and started saying
        // "something happened that this board has not heard about".
        if past_adp > falling {
            score.add(
                (past_adp * 0.15).min(8.0),
                format!("falling: {past_adp:.0} picks past ADP {adp:.0}"),
            );
        } else if past_adp < -ahead {
            let early = -past_adp;
            score.add(
                -(early * 0.1).min(6.0),
                format!("ahead of market: {early:.0} picks before ADP {adp:.0}"),
            );
        }
    }

    // An imported second opinion, in whichever direction it runs. Built next
    // to the rest of the reasons, in `second_opinion.rs`.
    if let Some((delta, reason)) =
        crate::second_opinion::rec_adjustment(p, ctx.inputs.teams, ctx.inputs.points_per_reception)
    {
        score.add(delta, reason);
    }

    if mode == Mode::Safe {
        // Volatile bonus-heavy value is not what safe mode is buying. Said out
        // loud: it is worth up to forty points and used to move the card
        // without ever appearing on it.
        let dependence = (p.bonus_points / p.points.max(1.0)) * 40.0;
        if dependence > 0.0 {
            let share = p.bonus_points / p.points.max(1.0) * 100.0;
            score.add(
                -dependence,
                format!("{share:.0}% of his points are yardage bonuses"),
            );
        }
        if let Some(adp) = p.adp {
            // Stay close to market: how far ahead of his own ADP this pick is,
            // at the pick actually being made. (The old form compared ADP with
            // his rank on *this* board and penalised bargains, never reaches.)
            let reach = (adp - ctx.inputs.market_pick as f64).max(0.0);
            // A fraction of a pick would read "0 picks ahead" — not a reason.
            let picks = reach.round();
            if picks >= 1.0 {
                score.add(
                    -reach * 0.3,
                    format!(
                        "a reach: {picks:.0} {} ahead of ADP {adp:.0}",
                        if picks == 1.0 { "pick" } else { "picks" }
                    ),
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
