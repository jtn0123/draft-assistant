//! The season screen's half of `chat_context.rs`: the week, the lineup, the
//! calls, the waivers and the table.
//!
//! Its own file so `chat_context.rs` stays inside the line cap. Included from
//! there as a private module and re-exported, so callers still find it under
//! `crate::chat_context`.

use super::{sanitise, tag, SplitContext};

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
            sanitise(&row.my_name),
            tag(&row.my_injury),
            row.my_points,
            sanitise(&row.opp_name),
            tag(&row.opp_injury),
            row.opp_points
        ));
    }
    out.push_str(&format!(
        "Your lineup as set projects {:.1} against a best of {:.1}, {:.1} left on the table.\n",
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
        .map(|set| {
            format!(
                "{} {}{}",
                set.slot,
                sanitise(&set.my_name),
                tag(&set.my_injury)
            )
        })
        .collect();
    if !benched.is_empty() {
        out.push_str(&format!(
            "Started but not in the best lineup: {}\n",
            benched.join(", ")
        ));
    }
    out
}

/// The season screen's context, split the same way the draft's is.
///
/// The league line is the only thing a week does not rewrite. Everything else
/// changes when the projections refresh, and used to sit in the cached prefix,
/// so a refresh in the middle of a conversation threw the whole cached thread
/// away. It travels after the history now, like the draft board does.
pub fn season_split(view: &crate::season::SeasonView) -> SplitContext {
    SplitContext {
        stable: season_stable(view),
        volatile: season_week(view),
    }
}

/// The season screen's equivalent context, both halves together.
pub fn season_context(view: &crate::season::SeasonView) -> String {
    season_split(view).joined()
}

fn season_stable(view: &crate::season::SeasonView) -> String {
    format!(
        "League: {}, week {} of season {}\n",
        sanitise(&view.league.name),
        view.week,
        view.season
    )
}

fn season_week(view: &crate::season::SeasonView) -> String {
    let mut out = String::new();
    if let Some(matchup) = &view.matchup {
        out.push_str(&format!(
            "This week: {} ({:.1} projected) vs {} ({:.1} projected). Win odds {:.0}%, playoff odds {:.0}%.\n",
            sanitise(&matchup.my_name),
            matchup.my_projected,
            sanitise(&matchup.opp_name),
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
                "{}: start {} over {} for {:+.1}, {}\n",
                call.slot,
                sanitise(&call.player_in),
                sanitise(&call.player_out),
                call.gain,
                call.why
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
                        sanitise(&w.name),
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
                sanitise(&row.name),
                row.record,
                row.playoff_odds * 100.0
            ));
        }
    }
    out
}
