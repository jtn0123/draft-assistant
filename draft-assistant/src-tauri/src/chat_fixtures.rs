//! Views built by hand, for the chat context tests.
//!
//! `chat_context` turns a view into prompt text, and the text is the thing
//! under test: a fixture assembled here is small enough that a test can pin
//! the exact lines it produces. Test-only — `lib.rs` gates the module on
//! `cfg(test)`.

use crate::board::{AvailablePlayer, BoardPlayer};
use crate::draft::{RosterEntry, TeamRoster};
use crate::view::{DataHealth, DraftStatus, DraftView, LeagueSummary};
use std::collections::HashMap;

const TEAMS: u32 = 10;
const ROUNDS: u32 = 15;

fn player(rank: u32, name: &str, position: &str, points: f64) -> AvailablePlayer {
    AvailablePlayer {
        player: BoardPlayer {
            player_id: format!("p{rank}"),
            name: name.into(),
            position: position.into(),
            team: Some("SF".into()),
            bye_week: Some(9),
            points,
            bonus_points: 0.0,
            vorp: points - 100.0,
            tier: 1 + rank / 3,
            position_rank: rank,
            overall_rank: rank,
            adp: Some(f64::from(rank) + 0.4),
            injury_status: None,
            sleeper_pts_ppr: None,
            second_opinion: None,
            weekly_cv: None,
        },
        survival_next: Some(0.5 - f64::from(rank) / 100.0),
    }
}

/// A ten-team, fifteen-round snake with my slot at 3, two players on my
/// roster and three on the board. No keepers, no trades, no reversal.
pub fn draft_fixture() -> DraftView {
    DraftView {
        schema_version: "1.0".into(),
        generated_at: 1_700_000_000,
        league: LeagueSummary {
            league_id: "L1".into(),
            platform: "sleeper".into(),
            name: "The League".into(),
            season: "2026".into(),
            total_rosters: TEAMS,
            roster_positions: vec![
                "QB".into(),
                "RB".into(),
                "RB".into(),
                "FLEX".into(),
                "BN".into(),
                "BN".into(),
            ],
            draftable_positions: vec!["QB".into(), "RB".into(), "WR".into(), "TE".into()],
            // Half PPR with a passing-TD value, so the scoring line has
            // something real to report.
            scoring_settings: HashMap::from([
                ("rec".to_string(), 0.5),
                ("pass_td".to_string(), 4.0),
            ]),
        },
        draft: DraftStatus {
            draft_id: "D1".into(),
            status: "drafting".into(),
            teams: TEAMS,
            rounds: ROUNDS,
            pick_timer: Some(90),
            current_pick: 24,
            current_round: 3,
            on_clock_slot: 7,
            on_clock_name: Some("Dana".into()),
            my_slot: Some(3),
            is_my_pick: false,
            picks_until_mine: Some(4),
            my_next_picks: vec![28, 43, 48, 63, 68],
            total_picks_made: 23,
            manual_picks_active: false,
            clock_deadline_ms: None,
            pick_slot_overrides: HashMap::new(),
            keeper_picks: Vec::new(),
        },
        my_roster: Some(TeamRoster {
            slot: 3,
            display_name: Some("Me".into()),
            players: vec![
                RosterEntry {
                    player_id: "p1".into(),
                    name: "Bijan Robinson".into(),
                    position: "RB".into(),
                    team: Some("ATL".into()),
                    pick_no: 3,
                    round: 1,
                    is_keeper: false,
                },
                RosterEntry {
                    player_id: "p2".into(),
                    name: "Brock Bowers".into(),
                    position: "TE".into(),
                    team: Some("LV".into()),
                    pick_no: 18,
                    round: 2,
                    is_keeper: false,
                },
            ],
            open_starters: vec![("RB".into(), 1), ("FLEX".into(), 2)],
        }),
        rosters: Vec::new(),
        available: vec![
            player(11, "Ladd McConkey", "WR", 214.0),
            player(12, "Kyren Williams", "RB", 208.5),
            player(13, "Jayden Daniels", "QB", 301.0),
        ],
        tier_alerts: Vec::new(),
        position_run: None,
        recommendations: Vec::new(),
        recent_picks: Vec::new(),
        replacement_baselines: HashMap::new(),
        replacement_demand: HashMap::new(),
        pick_prices: Vec::new(),
        data_health: DataHealth {
            players_fetched_at: 0,
            projections_fetched_at: 0,
            weekly_fetched_at: 0,
            board_size: 3,
            warnings: Vec::new(),
            poll_last_success_at: None,
            poll_consecutive_failures: 0,
            poll_last_error: None,
            second_opinion_loaded_at: None,
        },
    }
}
