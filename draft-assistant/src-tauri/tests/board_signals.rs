//! The two board signals the draft screen shows above the player list: which
//! tier is about to run out at each position, and whether a run on a position
//! is under way.
//!
//! Both are recomputed on every poll tick, so both are written to touch the
//! board as little as possible. These tests pin the answers down so that
//! staying cheap cannot quietly change what the screen says.

use draft_assistant_lib::board::{AvailablePlayer, BoardPlayer};
use draft_assistant_lib::view::{position_run, tier_alerts};

fn player(position: &str, tier: u32) -> AvailablePlayer {
    AvailablePlayer {
        survival_next: None,
        player: BoardPlayer {
            player_id: format!("{position}-{tier}"),
            name: format!("{position} T{tier}"),
            position: position.to_string(),
            team: None,
            bye_week: None,
            points: 0.0,
            bonus_points: 0.0,
            vorp: 0.0,
            tier,
            position_rank: 0,
            overall_rank: 0,
            adp: None,
            injury_status: None,
            sleeper_pts_ppr: None,
        },
    }
}

fn positions(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn one_alert_per_position_in_the_order_the_league_rosters_them() {
    let board = [
        player("RB", 1),
        player("WR", 2),
        player("RB", 1),
        player("QB", 3),
    ];
    let alerts = tier_alerts(&board, positions(&["QB", "RB", "WR", "TE"]));
    let seen: Vec<(&str, u32, u32)> = alerts
        .iter()
        .map(|a| (a.position.as_str(), a.tier, a.players_left))
        .collect();
    // TE has nobody left, so there is nothing to say about it.
    assert_eq!(seen, vec![("QB", 3, 1), ("RB", 1, 2), ("WR", 2, 1)]);
}

#[test]
fn only_the_best_tier_left_is_counted_wherever_its_players_sit() {
    // The board is ranked, not tier-sorted: a tier-1 receiver can sit below a
    // tier-2 one. The alert is about the best tier left, so both count and the
    // tier-2 player in between does not.
    let board = [
        player("WR", 1),
        player("WR", 2),
        player("WR", 1),
        player("WR", 3),
    ];
    let alerts = tier_alerts(&board, positions(&["WR"]));
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].tier, 1);
    assert_eq!(alerts[0].players_left, 2);
}

#[test]
fn an_empty_board_raises_nothing() {
    assert!(tier_alerts(&[], positions(&["QB", "RB"])).is_empty());
}

#[test]
fn a_run_is_judged_on_the_last_six_picks_and_nothing_before_them() {
    let picks: Vec<String> = ["RB", "RB", "RB", "RB", "QB", "WR", "TE", "QB", "WR", "TE"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    // Four straight running backs, but they are ten picks ago: not a run now.
    assert_eq!(position_run(&picks, 6, 4), None);

    // Only the tail matters, so passing just the tail gives the same answer as
    // passing the whole draft — which is why the caller only looks up six.
    let tail: Vec<String> = picks[picks.len() - 6..].to_vec();
    assert_eq!(position_run(&tail, 6, 4), position_run(&picks, 6, 4));

    let run_now: Vec<String> = ["WR", "TE", "RB", "RB", "QB", "RB", "RB"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let tail: Vec<String> = run_now[run_now.len() - 6..].to_vec();
    assert_eq!(position_run(&tail, 6, 4), position_run(&run_now, 6, 4));
    assert_eq!(
        position_run(&run_now, 6, 4).map(|r| (r.position, r.count)),
        Some(("RB".to_string(), 4))
    );
}
