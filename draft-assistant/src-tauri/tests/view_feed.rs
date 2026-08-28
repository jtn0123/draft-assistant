//! The parts of the view that follow the live feed: the poll fingerprint that
//! decides when the UI is told about a change, and the pick clock.

#[cfg(test)]
mod poll_fingerprint_tests {
    use draft_assistant_lib::sleeper::{Draft, Pick};
    use draft_assistant_lib::view::poll_fingerprint;

    fn draft(status: &str, last_picked: Option<i64>) -> Draft {
        serde_json::from_value(serde_json::json!({
            "draft_id": "d", "status": status, "type": "snake",
            "settings": {"teams": 2, "rounds": 2}, "last_picked": last_picked
        }))
        .unwrap()
    }

    fn pick(pick_no: u32, player_id: &str) -> Pick {
        Pick {
            round: 1,
            pick_no,
            draft_slot: pick_no,
            player_id: player_id.into(),
            picked_by: None,
            metadata: None,
        }
    }

    // The loop used to emit only when the pick count or status changed, so a
    // commissioner undo + redo (same count, different player) stayed
    // invisible until the next pick landed.
    #[test]
    fn swapping_a_player_at_the_same_count_changes_the_fingerprint() {
        let before = poll_fingerprint(&[pick(1, "a"), pick(2, "b")], &draft("drafting", None));
        let after = poll_fingerprint(&[pick(1, "a"), pick(2, "c")], &draft("drafting", None));
        assert_ne!(before, after);
    }

    #[test]
    fn a_new_pick_clock_changes_the_fingerprint() {
        let picks = [pick(1, "a")];
        let before = poll_fingerprint(&picks, &draft("drafting", Some(1_000)));
        let after = poll_fingerprint(&picks, &draft("drafting", Some(2_000)));
        assert_ne!(before, after);
    }

    #[test]
    fn identical_feeds_share_a_fingerprint() {
        let picks = [pick(1, "a"), pick(2, "b")];
        assert_eq!(
            poll_fingerprint(&picks, &draft("drafting", Some(5))),
            poll_fingerprint(picks.as_ref(), &draft("drafting", Some(5)))
        );
        assert_ne!(
            poll_fingerprint(&picks, &draft("drafting", None)),
            poll_fingerprint(&picks, &draft("complete", None))
        );
    }
}

#[cfg(test)]
mod clock_tests {
    use draft_assistant_lib::board::BoardPlayer;
    use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
    use draft_assistant_lib::roster::RosterRules;
    use draft_assistant_lib::sleeper::{Draft, League, Pick};
    use draft_assistant_lib::valuation::ReplacementModel;
    use draft_assistant_lib::view::build_view;
    use std::collections::HashMap;

    fn loaded(draft: serde_json::Value, picks: Vec<Pick>) -> LoadedLeague {
        let league: League = serde_json::from_value(serde_json::json!({
            "league_id": "l1", "name": "Test", "season": "2026", "status": "drafting",
            "total_rosters": 2, "roster_positions": ["WR", "BN"], "scoring_settings": {},
            "draft_id": "d1"
        }))
        .unwrap();
        let draft: Draft = serde_json::from_value(draft).unwrap();
        let board: Vec<BoardPlayer> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| BoardPlayer {
                player_id: (*id).into(),
                name: (*id).into(),
                position: "WR".into(),
                team: None,
                bye_week: None,
                points: 100.0,
                bonus_points: 0.0,
                vorp: 10.0,
                tier: 1,
                position_rank: 1,
                overall_rank: 1,
                adp: None,
                injury_status: None,
                sleeper_pts_ppr: None,
            })
            .collect();
        let board_index = board
            .iter()
            .enumerate()
            .map(|(i, p)| (p.player_id.clone(), i))
            .collect();
        LoadedLeague {
            league,
            draft,
            user_names: HashMap::new(),
            board,
            board_index,
            replacement_model: ReplacementModel::default(),
            roster_rules: RosterRules::new(&["WR".into(), "BN".into()]),
            api_picks: picks,
            manual_picks: Vec::new(),
            poll_last_success_at: None,
            poll_consecutive_failures: 0,
            poll_last_error: None,
            players_fetched_at: 0,
            projections_fetched_at: 0,
            weekly_fetched_at: 0,
            warnings: Vec::new(),
            player_meta: HashMap::new(),
        }
    }

    fn pick(pick_no: u32, player_id: &str) -> Pick {
        Pick {
            round: (pick_no - 1) / 2 + 1,
            pick_no,
            draft_slot: 1,
            player_id: player_id.into(),
            picked_by: None,
            metadata: None,
        }
    }

    // Sleeper sends pick_timer, start_time, and last_picked; the banner showed
    // none of them, so a draft screen had no clock.
    #[test]
    fn a_live_draft_exposes_the_pick_deadline_from_last_picked_and_the_timer() {
        let view = build_view(
            &loaded(
                serde_json::json!({
                    "draft_id": "d1", "status": "drafting", "type": "snake",
                    "settings": {"teams": 2, "rounds": 2, "pick_timer": 90},
                    "start_time": 1_700_000_000_000i64, "last_picked": 1_700_000_100_000i64
                }),
                vec![pick(1, "a")],
            ),
            &AppConfig::default(),
        );
        assert_eq!(view.draft.start_time, Some(1_700_000_000_000));
        assert_eq!(view.draft.pick_deadline, Some(1_700_000_190_000));
    }

    #[test]
    fn the_first_pick_clock_runs_from_the_start_time() {
        let view = build_view(
            &loaded(
                serde_json::json!({
                    "draft_id": "d1", "status": "drafting", "type": "snake",
                    "settings": {"teams": 2, "rounds": 2, "pick_timer": 60},
                    "start_time": 1_700_000_000_000i64
                }),
                Vec::new(),
            ),
            &AppConfig::default(),
        );
        assert_eq!(view.draft.pick_deadline, Some(1_700_000_060_000));
    }

    #[test]
    fn no_deadline_before_the_draft_after_it_or_without_a_timer() {
        let pre = build_view(
            &loaded(
                serde_json::json!({
                    "draft_id": "d1", "status": "pre_draft", "type": "snake",
                    "settings": {"teams": 2, "rounds": 2, "pick_timer": 90},
                    "start_time": 1_700_000_000_000i64
                }),
                Vec::new(),
            ),
            &AppConfig::default(),
        );
        assert_eq!(pre.draft.pick_deadline, None);
        assert_eq!(pre.draft.start_time, Some(1_700_000_000_000));

        let done = build_view(
            &loaded(
                serde_json::json!({
                    "draft_id": "d1", "status": "drafting", "type": "snake",
                    "settings": {"teams": 2, "rounds": 2, "pick_timer": 90},
                    "last_picked": 1_700_000_100_000i64
                }),
                vec![pick(1, "a"), pick(2, "b"), pick(3, "c"), pick(4, "d")],
            ),
            &AppConfig::default(),
        );
        assert_eq!(done.draft.pick_deadline, None);

        let untimed = build_view(
            &loaded(
                serde_json::json!({
                    "draft_id": "d1", "status": "drafting", "type": "snake",
                    "settings": {"teams": 2, "rounds": 2, "pick_timer": 0},
                    "last_picked": 1_700_000_100_000i64
                }),
                vec![pick(1, "a")],
            ),
            &AppConfig::default(),
        );
        assert_eq!(untimed.draft.pick_deadline, None);
    }
}
