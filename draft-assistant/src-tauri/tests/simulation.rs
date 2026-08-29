use draft_assistant_lib::board::{AvailablePlayer, BoardPlayer};
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::simulation::apply_simulated_pick;
use draft_assistant_lib::sleeper::{Draft, DraftSettings, League, PlayerMeta};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::view::build_view;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct Fixture {
    league: FixtureLeague,
    draft: FixtureDraft,
    available: Vec<AvailablePlayer>,
    replacement_baselines: HashMap<String, f64>,
    replacement_demand: HashMap<String, usize>,
}

#[derive(Deserialize)]
struct FixtureLeague {
    league_id: String,
    name: String,
    season: String,
    total_rosters: u32,
    roster_positions: Vec<String>,
    scoring_settings: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct FixtureDraft {
    draft_id: String,
    teams: u32,
    rounds: u32,
    pick_timer: Option<u32>,
    my_slot: Option<u32>,
}

fn loaded_fixture() -> (LoadedLeague, AppConfig) {
    let fixture: Fixture = serde_json::from_str(include_str!("../../public/dev-fixture.json"))
        .expect("development fixture must match the serialized player contract");
    let my_user_id = "simulation-user".to_string();
    let my_slot = fixture.draft.my_slot.unwrap_or(1);
    let draft_order = HashMap::from([(my_user_id.clone(), my_slot)]);
    let board: Vec<BoardPlayer> = fixture
        .available
        .into_iter()
        .map(|available| available.player)
        .collect();
    let board_index = board
        .iter()
        .enumerate()
        .map(|(index, player)| (player.player_id.clone(), index))
        .collect();
    let player_meta = board
        .iter()
        .map(|player| {
            (
                player.player_id.clone(),
                PlayerMeta {
                    full_name: Some(player.name.clone()),
                    first_name: None,
                    last_name: None,
                    position: Some(player.position.clone()),
                    team: player.team.clone(),
                    fantasy_positions: None,
                    injury_status: player.injury_status.clone(),
                    years_exp: None,
                    age: None,
                },
            )
        })
        .collect();
    let roster_rules = RosterRules::new(&fixture.league.roster_positions);
    let league = League {
        league_id: fixture.league.league_id,
        name: fixture.league.name,
        season: fixture.league.season,
        status: "pre_draft".into(),
        total_rosters: fixture.league.total_rosters,
        roster_positions: fixture.league.roster_positions,
        scoring_settings: fixture.league.scoring_settings,
        draft_id: Some(fixture.draft.draft_id.clone()),
        settings: Default::default(),
        previous_league_id: None,
    };
    let draft = Draft {
        draft_id: fixture.draft.draft_id,
        status: "drafting".into(),
        draft_type: "snake".into(),
        settings: DraftSettings {
            teams: fixture.draft.teams,
            rounds: fixture.draft.rounds,
            pick_timer: fixture.draft.pick_timer,
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
        draft_order: Some(draft_order),
        start_time: None,
        last_picked: None,
        season: Some(league.season.clone()),
        metadata: None,
        creators: None,
        slot_to_roster_id: None,
    };
    let config = AppConfig {
        my_user_id: Some(my_user_id.clone()),
        active_league_id: Some(league.league_id.clone()),
        leagues: Vec::new(),
    };
    let loaded = LoadedLeague {
        league,
        draft,
        user_names: HashMap::from([(my_user_id, "Simulation User".into())]),
        board,
        board_index,
        replacement_model: ReplacementModel {
            baseline: fixture.replacement_baselines,
            demand: fixture.replacement_demand,
        },
        roster_rules,
        api_picks: Vec::new(),
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        weekly_points: HashMap::new(),
        nfl_state: None,
        matchups: Vec::new(),
        trending: Vec::new(),
        league_rosters: Vec::new(),
        past_matchups: Vec::new(),
        transactions: Vec::new(),
        schedule: Vec::new(),
        history: None,
        keeper_pick_nos: Default::default(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 0,
        projections_fetched_at: 0,
        weekly_fetched_at: 0,
        warnings: Vec::new(),
        player_meta,
    };
    (loaded, config)
}

#[test]
fn full_draft_simulation_preserves_view_invariants() {
    let (mut loaded, config) = loaded_fixture();
    let total_picks = loaded.draft.settings.teams * loaded.draft.settings.rounds;

    for pick_no in 1..=total_picks {
        let before = build_view(&loaded, &config);
        assert_eq!(before.rosters.len(), loaded.draft.settings.teams as usize);
        assert!(!before.recommendations.is_empty(), "pick {pick_no}");
        assert!(before.available.iter().all(|player| player
            .survival_next
            .is_none_or(|probability| (0.0..=1.0).contains(&probability))));

        let selected = apply_simulated_pick(&mut loaded, &config)
            .unwrap_or_else(|| panic!("simulation ran out of candidates at pick {pick_no}"));
        let after = build_view(&loaded, &config);
        assert!(after
            .available
            .iter()
            .all(|player| player.player.player_id != selected));
        let unique: HashSet<&str> = loaded
            .manual_picks
            .iter()
            .map(|pick| pick.player_id.as_str())
            .collect();
        assert_eq!(unique.len(), loaded.manual_picks.len(), "pick {pick_no}");
    }

    let final_view = build_view(&loaded, &config);
    assert_eq!(final_view.draft.total_picks_made, total_picks as usize);
    assert!(final_view.available.iter().all(|available| !loaded
        .manual_picks
        .iter()
        .any(|pick| pick.player_id == available.player.player_id)));
    assert!(final_view
        .rosters
        .iter()
        .all(|roster| roster.players.len() == loaded.draft.settings.rounds as usize));
    assert!(final_view
        .my_roster
        .as_ref()
        .is_some_and(|roster| roster.open_starters.is_empty()));
}

/// The UI orders live updates on `seq` and drops anything not newer, so a
/// non-increasing `seq` would make the board silently stop updating.
#[test]
fn view_seq_strictly_increases_across_builds() {
    let (mut loaded, config) = loaded_fixture();
    let mut last = 0;
    for _ in 0..5 {
        let view = build_view(&loaded, &config);
        assert!(
            view.seq > last,
            "seq must strictly increase: got {} after {last}",
            view.seq
        );
        last = view.seq;
        loaded.manual_picks.clear();
    }
}
