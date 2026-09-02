//! Turning a view into the context Claude actually needs.
//!
//! The full `DraftView` runs to hundreds of kilobytes, nearly all of it board
//! rows past the point of usefulness. These summarisers keep the head of the
//! board and everything situational, which is what makes the system prompt
//! small enough to cache.

use crate::chat_rules::{league_rules, LeagueRules};

/// A comma-separated pick list, clipped so a manager who traded half a draft
/// away cannot push the board out of the prompt.
fn pick_list(picks: &[u32]) -> String {
    let shown = picks
        .iter()
        .take(8)
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if picks.len() > 8 {
        format!("{shown} and {} more", picks.len() - 8)
    } else {
        shown
    }
}

/// The league's house rules, as the lines Claude reads them on. Empty for an
/// ordinary snake with no keepers and no trades — most leagues, most nights.
fn rules_lines(rules: &LeagueRules) -> String {
    let mut out = String::new();
    if rules.keepers_total > 0 {
        out.push_str(&format!(
            "Keepers: {} picks league-wide are already spent",
            rules.keepers_total
        ));
        if rules.my_keeper_picks.is_empty() {
            out.push_str(" — none of them yours.\n");
        } else {
            out.push_str(&format!(
                " — yours at {}.\n",
                pick_list(&rules.my_keeper_picks)
            ));
        }
    }
    if !rules.picks_gained.is_empty() || !rules.picks_lost.is_empty() {
        out.push_str("Traded picks: ");
        let mut halves = Vec::new();
        if !rules.picks_gained.is_empty() {
            halves.push(format!("you gained {}", pick_list(&rules.picks_gained)));
        }
        if !rules.picks_lost.is_empty() {
            halves.push(format!("you lost {}", pick_list(&rules.picks_lost)));
        }
        out.push_str(&halves.join("; "));
        out.push_str(".\n");
    }
    if let Some(round) = rules.reversal_round {
        out.push_str(&format!(
            "Third-round reversal: the order flips at round {round} instead of snaking, so it repeats the round before.\n"
        ));
    }
    out
}

/// The draft screen's context block.
pub fn draft_context(view: &crate::view::DraftView) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "League: {} ({} teams, {} rounds, season {})\n",
        view.league.name, view.draft.teams, view.draft.rounds, view.league.season
    ));
    out.push_str(&format!(
        "Now: round {}, pick {}, on the clock {}. Your slot: {}. Your next picks: {:?}\n",
        view.draft.current_round,
        view.draft.current_pick,
        view.draft.on_clock_name.as_deref().unwrap_or("unknown"),
        view.draft
            .my_slot
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".into()),
        view.draft.my_next_picks.iter().take(4).collect::<Vec<_>>()
    ));
    out.push_str(&rules_lines(&league_rules(view)));

    if let Some(roster) = &view.my_roster {
        out.push_str("Your roster: ");
        out.push_str(
            &roster
                .players
                .iter()
                .map(|p| format!("{} {} (R{})", p.position, p.name, p.round))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
        out.push_str(&format!(
            "Open starters: {}\n",
            roster
                .open_starters
                .iter()
                .map(|(slot, n)| format!("{slot}x{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    out.push_str("\nBest available (rank, name, pos, pts, VORP, tier, ADP, survival):\n");
    for player in view.available.iter().take(40) {
        out.push_str(&format!(
            "{}. {} {} — {:.0} pts, VORP {:.0}, T{}, ADP {}, survives {}\n",
            player.player.overall_rank,
            player.player.name,
            player.player.position,
            player.player.points,
            player.player.vorp,
            player.player.tier,
            player
                .player
                .adp
                .map(|a| format!("{a:.0}"))
                .unwrap_or_else(|| "-".into()),
            player
                .survival_next
                .map(|s| format!("{:.0}%", s * 100.0))
                .unwrap_or_else(|| "-".into()),
        ));
    }

    if !view.tier_alerts.is_empty() {
        out.push_str("\nTier alerts: ");
        out.push_str(
            &view
                .tier_alerts
                .iter()
                .map(|a| format!("{} T{} has {} left", a.position, a.tier, a.players_left))
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push('\n');
    }
    if let Some(run) = &view.position_run {
        out.push_str(&format!(
            "Position run in progress: {} ({} of the last {} picks)\n",
            run.position, run.count, run.window
        ));
    }
    if !view.pick_prices.is_empty() {
        out.push_str("Round prices so far (points over replacement the round actually took): ");
        out.push_str(
            &view
                .pick_prices
                .iter()
                .take(10)
                .map(|p| format!("R{} {:.0}", p.round, p.points))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    if !view.recent_picks.is_empty() {
        out.push_str("Recent picks: ");
        out.push_str(
            &view
                .recent_picks
                .iter()
                .take(8)
                .map(|p| format!("{} {} ({})", p.pick_no, p.name, p.position))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    out
}

/// "(Q)" after a name, and nothing at all for a player with no tag.
fn tag(injury: &Option<String>) -> String {
    match injury {
        Some(code) if !code.is_empty() => format!(" ({code})"),
        _ => String::new(),
    }
}

/// The head-to-head lineup, both sides tagged with this week's injuries, and
/// what the lineup that is actually set gives up against the best one. The
/// distinction matters: the rows below are the *best* lineup, so without this
/// Claude would read a start/sit recommendation as already taken.
pub(crate) fn lineup_block(matchup: &crate::season::MatchupView, points_on_table: f64) -> String {
    let mut out =
        String::from("Best lineup (slot, yours, proj, theirs, proj; Q/D/O = injury tag):\n");
    for row in &matchup.rows {
        out.push_str(&format!(
            "{}: {}{} {:.1} vs {}{} {:.1}\n",
            row.slot,
            row.my_name,
            tag(&row.my_injury),
            row.my_points,
            row.opp_name,
            tag(&row.opp_injury),
            row.opp_points
        ));
    }
    out.push_str(&format!(
        "Your lineup as set projects {:.1} against a best of {:.1} — {:.1} left on the table.\n",
        matchup.set_projected, matchup.my_projected, points_on_table
    ));
    let benched: Vec<String> = matchup
        .set_rows
        .iter()
        .filter(|set| {
            matchup
                .rows
                .iter()
                .all(|best| best.my_player_id != set.my_player_id)
        })
        .map(|set| format!("{} {}{}", set.slot, set.my_name, tag(&set.my_injury)))
        .collect();
    if !benched.is_empty() {
        out.push_str(&format!(
            "Started but not in the best lineup: {}\n",
            benched.join(", ")
        ));
    }
    out
}

/// The season screen's equivalent context.
pub fn season_context(view: &crate::season::SeasonView) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "League: {} — week {} of season {}\n",
        view.league.name, view.week, view.season
    ));
    if let Some(matchup) = &view.matchup {
        out.push_str(&format!(
            "This week: {} ({:.1} projected) vs {} ({:.1} projected). Win odds {:.0}%, playoff odds {:.0}%.\n",
            matchup.my_name,
            matchup.my_projected,
            matchup.opp_name,
            matchup.opp_projected,
            view.header.win_odds_best * 100.0,
            view.header.playoff_odds * 100.0
        ));
        out.push_str(&lineup_block(matchup, view.points_on_table));
    }
    if !view.calls.is_empty() {
        out.push_str("\nStart/sit calls available:\n");
        for call in &view.calls {
            out.push_str(&format!(
                "{}: start {} over {} for {:+.1} — {}\n",
                call.slot, call.player_in, call.player_out, call.gain, call.why
            ));
        }
    }
    if !view.waivers.is_empty() {
        out.push_str("\nWaiver targets: ");
        out.push_str(
            &view
                .waivers
                .iter()
                .map(|w| {
                    format!(
                        "{} {} (+{:.1}/wk, suggest ${})",
                        w.position,
                        w.name,
                        w.gain_points,
                        w.suggested_bid
                            .map(|b| b.to_string())
                            .unwrap_or_else(|| "-".into())
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push('\n');
    }
    if !view.standings.is_empty() {
        out.push_str("\nStandings (seed, team, record, playoff odds):\n");
        for row in &view.standings {
            out.push_str(&format!(
                "{}. {} {} {:.0}%\n",
                row.seed,
                row.name,
                row.record,
                row.playoff_odds * 100.0
            ));
        }
    }
    out
}

/// The lines these functions produce, pinned. Its own file only to keep this
/// one inside the line cap.
#[cfg(test)]
#[path = "chat_context_tests.rs"]
mod context_tests;

/// The blocks that only appear when the draft has something to say.
#[cfg(test)]
#[path = "chat_context_extras_tests.rs"]
mod extras_tests;

/// Suggested prompts shown under the thread, tailored to the screen.
pub fn suggestions(screen: &str) -> Vec<String> {
    let items: &[&str] = if screen == "season" {
        &[
            "Who should I start this week?",
            "Is my playoff path realistic?",
            "Which waiver claim matters most?",
        ]
    } else {
        &[
            "Who's left at TE?",
            "Am I thin at RB?",
            "Best value at my next pick?",
        ]
    };
    items.iter().map(|s| (*s).to_string()).collect()
}
