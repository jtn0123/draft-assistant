//! A deterministic four-team league fixture shared by the season/state/chat
//! integration tests. Everything is hand-built: no network, no Tauri runtime.

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::season_api::{
    GameMeta, Matchup, Roster, RosterSettings, ScoreGame, TradedPick, Transaction,
    TransactionSettings,
};
use draft_assistant_lib::season_engine::LoadedSeason;
use draft_assistant_lib::season_history::{History, PlayerSnap, Snapshot, TeamSnap};
use draft_assistant_lib::season_types::LastSeasonRow;
use draft_assistant_lib::sleeper::{
    Draft, DraftSettings, League, LeagueSettings, PlayerMeta, ProjectionRow,
};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::weekly::WeeklyPoints;
use std::collections::HashMap;

/// Week the fixture season sits in.
const WEEK: u32 = 2;
/// Kickoff of the fixture's still-upcoming game, epoch milliseconds.
const FUTURE_KICKOFF_MS: i64 = 4_000_000_000_000;
/// First trend snapshot, epoch seconds.
const SNAP_AT: u64 = 1_700_000_000;
/// Second trend snapshot, epoch seconds.
const SNAP_AT_2: u64 = SNAP_AT + 21_600;

fn board_player(id: &str, name: &str, position: &str, team: &str, rank: u32) -> BoardPlayer {
    BoardPlayer {
        player_id: id.to_string(),
        name: name.to_string(),
        position: position.to_string(),
        team: Some(team.to_string()),
        bye_week: Some(9),
        points: 300.0 - rank as f64,
        bonus_points: 1.0,
        vorp: 100.0 - rank as f64,
        tier: 1 + rank / 4,
        position_rank: rank,
        overall_rank: rank,
        adp: if id == "w5" { None } else { Some(rank as f64) },
        injury_status: None,
        sleeper_pts_ppr: None,
    }
}

fn proj(id: &str, week: u32, rush_yd: f64) -> ProjectionRow {
    ProjectionRow {
        player_id: id.to_string(),
        stats: Some(HashMap::from([("rush_yd".to_string(), rush_yd)])),
        player: None,
        week: Some(week),
        opponent: None,
    }
}

fn roster(
    id: u32,
    owner: &str,
    players: &[&str],
    starters: &[&str],
    wins: u32,
    fpts: f64,
) -> Roster {
    Roster {
        roster_id: id,
        owner_id: Some(owner.to_string()),
        players: Some(players.iter().map(|p| (*p).to_string()).collect()),
        starters: Some(starters.iter().map(|p| (*p).to_string()).collect()),
        reserve: None,
        settings: RosterSettings {
            wins,
            losses: 2 - wins,
            fpts,
            waiver_budget_used: if id == 1 { 25.0 } else { 0.0 },
            ..RosterSettings::default()
        },
    }
}

fn matchup(roster_id: u32, matchup_id: u32, starters: &[&str]) -> Matchup {
    Matchup {
        roster_id,
        matchup_id: Some(matchup_id),
        points: 0.0,
        custom_points: None,
        starters: Some(starters.iter().map(|s| (*s).to_string()).collect()),
        players: None,
        players_points: None,
    }
}

fn snap_team(roster_id: u32, strength: f64, players: &[(&str, f64)]) -> TeamSnap {
    TeamSnap {
        roster_id,
        strength,
        players: players
            .iter()
            .map(|(id, points)| {
                (
                    (*id).to_string(),
                    PlayerSnap {
                        points: *points,
                        injury: None,
                    },
                )
            })
            .collect(),
    }
}

/// Every player on the fixture board: (id, name, position, NFL team).
const PLAYERS: &[(&str, &str, &str, &str)] = &[
    ("q1", "Ace Passer", "QB", "ATL"),
    ("r1", "Lead Back", "RB", "ATL"),
    ("w1", "Alpha Wideout", "WR", "TB"),
    ("w2", "Slot Wideout", "WR", "TB"),
    ("r2", "Bench Back", "RB", "TB"),
    ("w5", "Bye Wideout", "WR", "DAL"),
    ("q2", "Rival Passer", "QB", "PIT"),
    ("r3", "Rival Back", "RB", "PIT"),
    ("w3", "Rival Wideout", "WR", "BAL"),
    ("w4", "Rival Slot", "WR", "BAL"),
    ("q3", "Third Passer", "QB", "SF"),
    ("r5", "Third Back", "RB", "SF"),
    ("w6", "Third Wideout", "WR", "SEA"),
    ("q4", "Fourth Passer", "QB", "KC"),
    ("r6", "Fourth Back", "RB", "KC"),
    ("w7", "Fourth Wideout", "WR", "DEN"),
    ("w8", "Fourth Slot", "WR", "DEN"),
    ("fa1", "Waiver Back", "RB", "LV"),
    ("fa2", "Roster Filler", "WR", "LV"),
];

/// Projected rushing yards per (player, week); points are a tenth of these.
fn projections() -> Vec<ProjectionRow> {
    let mut rows = Vec::new();
    let per_week: &[(&str, f64)] = &[
        ("q1", 180.0),
        ("r1", 150.0),
        ("w1", 120.0),
        ("w2", 80.0),
        ("r2", 100.0),
        ("q2", 170.0),
        ("r3", 140.0),
        ("w3", 110.0),
        ("w4", 90.0),
        ("q3", 160.0),
        ("r5", 130.0),
        ("w6", 100.0),
        ("q4", 150.0),
        ("r6", 120.0),
        ("w7", 95.0),
        ("w8", 60.0),
        ("fa1", 130.0),
    ];
    for (id, yards) in per_week {
        for week in 1..=3u32 {
            rows.push(proj(id, week, *yards));
        }
    }
    // w5 projects in weeks 1 and 3 only: on bye in week 2.
    rows.push(proj("w5", 1, 90.0));
    rows.push(proj("w5", 3, 90.0));
    rows
}

pub fn fixture() -> (LoadedLeague, LoadedSeason, AppConfig) {
    let scoring: HashMap<String, f64> = HashMap::from([("rush_yd".to_string(), 0.1)]);
    let roster_positions: Vec<String> = ["QB", "RB", "WR", "FLEX", "BN", "BN"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let board: Vec<BoardPlayer> = PLAYERS
        .iter()
        .enumerate()
        .map(|(i, (id, name, pos, team))| board_player(id, name, pos, team, i as u32 + 1))
        .collect();
    let board_index = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    let player_meta: HashMap<String, PlayerMeta> = HashMap::new();
    let league = League {
        league_id: "league-1".into(),
        name: "Fixture League".into(),
        season: "2025".into(),
        status: "in_season".into(),
        total_rosters: 4,
        roster_positions: roster_positions.clone(),
        scoring_settings: scoring.clone(),
        draft_id: Some("draft-1".into()),
        previous_league_id: None,
        settings: LeagueSettings {
            playoff_week_start: Some(4),
            playoff_teams: Some(2),
            waiver_budget: Some(100.0),
            start_week: Some(1),
        },
    };
    let draft = Draft {
        draft_id: "draft-1".into(),
        status: "drafting".into(),
        draft_type: "snake".into(),
        settings: DraftSettings {
            teams: 4,
            rounds: 6,
            pick_timer: None,
            reversal_round: None,
            slots_qb: None,
            slots_rb: None,
            slots_wr: None,
            slots_te: None,
            slots_flex: None,
            slots_super_flex: None,
            slots_k: None,
            slots_def: None,
        },
        draft_order: Some(HashMap::from([
            ("u1".to_string(), 1),
            ("u2".to_string(), 2),
            ("u3".to_string(), 3),
            ("u4".to_string(), 4),
        ])),
        start_time: None,
        season: Some("2025".into()),
        metadata: None,
        creators: None,
        last_picked: None,
        slot_to_roster_id: None,
    };
    let loaded = LoadedLeague {
        league,
        draft,
        user_names: HashMap::from([
            ("u1".to_string(), "User One".to_string()),
            ("u2".to_string(), "User Two".to_string()),
            ("u3".to_string(), "User Three".to_string()),
            ("u4".to_string(), "User Four".to_string()),
        ]),
        user_avatars: HashMap::from([
            ("u1".to_string(), "avatar-one".to_string()),
            ("u2".to_string(), "avatar-two".to_string()),
        ]),
        board,
        board_index,
        replacement_model: ReplacementModel {
            baseline: HashMap::new(),
            demand: HashMap::new(),
        },
        roster_rules: RosterRules::new(&roster_positions),
        api_picks: Vec::new(),
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        keeper_pick_nos: Default::default(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 1,
        projections_fetched_at: 2,
        weekly_fetched_at: 3,
        warnings: Vec::new(),
        player_meta,
        weekly_points: WeeklyPoints::build(&projections(), &scoring),
    };

    let season = LoadedSeason {
        week: WEEK,
        season: 2025,
        rosters: vec![
            roster(
                1,
                "u1",
                &["q1", "r1", "w1", "w2", "r2", "w5"],
                &["q1", "r1", "w1", "w2"],
                2,
                250.0,
            ),
            roster(
                2,
                "u2",
                &["q2", "r3", "w3", "w4"],
                &["q2", "r3", "w3", "w4"],
                1,
                230.0,
            ),
            roster(
                3,
                "u3",
                &["q3", "r5", "w6"],
                &["q3", "r5", "w6", "0"],
                1,
                210.0,
            ),
            roster(
                4,
                "u4",
                &["q4", "r6", "w7", "w8"],
                &["q4", "r6", "w7", "w8"],
                0,
                190.0,
            ),
        ],
        matchups: vec![
            {
                let mut m = matchup(1, 1, &["q1", "r1", "w1", "w2"]);
                m.players_points = Some(HashMap::from([("q1".to_string(), 21.5)]));
                m
            },
            matchup(2, 1, &["q2", "r3", "w3", "w4"]),
            matchup(3, 2, &["q3", "r5", "w6", "0"]),
            matchup(4, 2, &["q4", "r6", "w7", "w8"]),
        ],
        schedule: vec![
            (1, vec![(1, 2), (3, 4)]),
            (2, vec![(1, 2), (3, 4)]),
            (3, vec![(1, 4), (2, 3)]),
        ],
        season_points: HashMap::from([
            ("q1".to_string(), 40.0),
            ("r1".to_string(), 30.0),
            ("w1".to_string(), 25.0),
            ("w2".to_string(), 20.0),
            ("r2".to_string(), 22.0),
        ]),
        transactions: vec![
            Transaction {
                transaction_id: "trade-1".into(),
                kind: "trade".into(),
                status: "complete".into(),
                created: (SNAP_AT + 12_000) as i64 * 1000,
                adds: Some(HashMap::from([
                    ("w3".to_string(), 1),
                    ("w1".to_string(), 2),
                ])),
                drops: Some(HashMap::from([
                    ("w3".to_string(), 2),
                    ("w1".to_string(), 1),
                ])),
                roster_ids: vec![1, 2],
                // A wideout swap with a future second going the other way.
                draft_picks: vec![TradedPick {
                    season: "2027".into(),
                    round: 2,
                    owner_id: Some(2),
                }],
                settings: None,
            },
            Transaction {
                transaction_id: "waiver-1".into(),
                kind: "waiver".into(),
                status: "complete".into(),
                created: (SNAP_AT + 10_000) as i64 * 1000,
                adds: Some(HashMap::from([("fa2".to_string(), 2)])),
                drops: None,
                roster_ids: vec![2],
                draft_picks: Vec::new(),
                settings: Some(TransactionSettings {
                    waiver_bid: Some(12),
                }),
            },
        ],
        scores: vec![
            ScoreGame {
                game_id: Some("g-live".into()),
                status: Some("in_game".into()),
                start_time: Some(1_700_000_000_000),
                week: Some(WEEK),
                metadata: Some(GameMeta {
                    home_team: Some("ATL".into()),
                    away_team: Some("TB".into()),
                    home_score: Some(14),
                    away_score: Some(10),
                    quarter_num: Some(2),
                    time_remaining: Some("05:00".into()),
                    has_started: true,
                    is_in_progress: true,
                    ..GameMeta::default()
                }),
            },
            ScoreGame {
                game_id: Some("g-pre".into()),
                status: Some("pre_game".into()),
                start_time: Some(FUTURE_KICKOFF_MS),
                week: Some(WEEK),
                metadata: Some(GameMeta {
                    home_team: Some("BAL".into()),
                    away_team: Some("PIT".into()),
                    channel: Some("CBS".into()),
                    ..GameMeta::default()
                }),
            },
        ],
        last_season: vec![LastSeasonRow {
            place: 1,
            name: "User Two".into(),
            record: "10\u{2013}4".into(),
            points: 1500.5,
            tag: Some("Champ".into()),
            is_mine: false,
        }],
        history: History {
            snapshots: vec![
                Snapshot {
                    taken_at: SNAP_AT,
                    week: WEEK,
                    teams: vec![
                        snap_team(1, 50.0, &[("r2", 10.0)]),
                        snap_team(2, 51.0, &[]),
                        snap_team(3, 45.0, &[]),
                        snap_team(4, 40.0, &[]),
                    ],
                },
                Snapshot {
                    taken_at: SNAP_AT_2,
                    week: WEEK,
                    teams: vec![
                        snap_team(1, 55.0, &[("r2", 15.0)]),
                        snap_team(2, 51.2, &[("fa2", 4.0)]),
                        snap_team(3, 45.0, &[]),
                        snap_team(4, 40.0, &[]),
                    ],
                },
            ],
        },
        fetched_at: SNAP_AT_2,
        warnings: vec!["fixture warning".into()],
        sources: Default::default(),
    };

    let config = AppConfig {
        my_user_id: Some("u1".into()),
        active_league_id: Some("league-1".into()),
        leagues: Vec::new(),
        anthropic_api_key: None,
        chat_provider: None,
    };
    (loaded, season, config)
}
