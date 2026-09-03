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

/// How this league scores, in the three settings that actually change who is
/// worth what. Without them Claude reads every board through full-PPR habits
/// and argues for receivers in a league that pays nothing for a catch.
fn scoring_line(league: &crate::view::LeagueSummary) -> String {
    let setting = |key: &str| league.scoring_settings.get(key).copied().unwrap_or(0.0);
    let rec = setting("rec");
    let format = if rec >= 0.75 {
        "full PPR"
    } else if rec >= 0.25 {
        "half PPR"
    } else {
        "standard, no PPR"
    };
    let mut out = format!("Scoring: {format} ({rec:.2} per catch)");
    let te_premium = setting("bonus_rec_te");
    if te_premium > 0.0 {
        out.push_str(&format!(", TE premium +{te_premium:.2} per catch"));
    }
    out.push_str(&format!(", {:.0} per passing TD", setting("pass_td")));
    out.push('\n');
    out
}

/// The starting lineup this league runs, counted rather than listed: fifteen
/// slot labels in a row is noise, "QBx1 RBx2 WRx2 TEx1 FLEXx2" is the shape.
fn roster_shape(league: &crate::view::LeagueSummary) -> String {
    let mut order: Vec<&str> = Vec::new();
    let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for slot in &league.roster_positions {
        let count = counts.entry(slot.as_str()).or_insert_with(|| {
            order.push(slot.as_str());
            0
        });
        *count += 1;
    }
    let shape = order
        .iter()
        .map(|slot| format!("{slot}x{}", counts[slot]))
        .collect::<Vec<_>>()
        .join(" ");
    format!("Roster: {shape}\n")
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
    out.push_str(&scoring_line(&view.league));
    out.push_str(&roster_shape(&view.league));
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

    // Kickers and defences are left out until the draft is nearly over. Their
    // value over replacement is real but it is available to anybody in the
    // last two rounds, and listing them among the best available invited an
    // argument for taking one in the eighth.
    let late_rounds = view.draft.current_round + 2 >= view.draft.rounds;
    out.push_str(
        "\nBest available (rank, name, pos, pts, VORP, tier, ADP, survival, bye, injury):\n",
    );
    for player in view
        .available
        .iter()
        .filter(|p| late_rounds || !crate::board::is_late_only(&p.player.position))
        .take(40)
    {
        out.push_str(&format!(
            "{}. {} {} — {:.0} pts, VORP {:.0}, T{}, ADP {}, survives {}, bye {}{}\n",
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
            player
                .player
                .bye_week
                .map(|w| w.to_string())
                .unwrap_or_else(|| "-".into()),
            tag(&player.player.injury_status),
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
    // A round whose median pick was a below-replacement body prices at zero,
    // because the price is clamped there. "R11 0, R12 0, R13 0" is not a
    // price list, and Claude read it as those rounds being worthless rather
    // than as the floor it is — so the rounds that priced at nothing are left
    // out and the ones that priced at something speak for themselves.
    let priced: Vec<String> = view
        .pick_prices
        .iter()
        .filter(|p| p.points > 0.0)
        .take(10)
        .map(|p| format!("R{} {:.0}", p.round, p.points))
        .collect();
    if !priced.is_empty() {
        out.push_str("Round prices so far (points over replacement the round actually took): ");
        out.push_str(&priced.join(", "));
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
        // A slot the manager left empty is not a benched player: it has no id
        // and no name, and it used to come out as a blank entry in this list.
        .filter(|set| set.my_player_id.is_some())
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
