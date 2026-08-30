//! Turning a view into the context Claude actually needs.
//!
//! The full `DraftView` runs to hundreds of kilobytes, nearly all of it board
//! rows past the point of usefulness. These summarisers keep the head of the
//! board and everything situational, which is what makes the system prompt
//! small enough to cache.

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
            view.header.win_odds * 100.0,
            view.header.playoff_odds * 100.0
        ));
        out.push_str("Lineup (slot, yours, proj, theirs, proj):\n");
        for row in &matchup.rows {
            out.push_str(&format!(
                "{}: {} {:.1} vs {} {:.1}\n",
                row.slot, row.my_name, row.my_points, row.opp_name, row.opp_points
            ));
        }
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
