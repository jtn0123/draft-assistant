//! Snake-draft state: pick order math, per-team rosters, on-clock tracking,
//! and ADP-based survival probabilities.

use crate::roster::RosterRules;
use crate::sleeper::{Draft, Pick};
use serde::Serialize;

/// How pick numbers map to slots. Sleeper reports the type on the draft
/// (`snake` / `linear` / `auction`) and, for snake drafts, an optional
/// `reversal_round` from which the order reverses a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftOrder {
    pub linear: bool,
    /// 0 = plain snake.
    pub reversal_round: u32,
}

impl DraftOrder {
    pub const SNAKE: DraftOrder = DraftOrder {
        linear: false,
        reversal_round: 0,
    };

    pub const LINEAR: DraftOrder = DraftOrder {
        linear: true,
        reversal_round: 0,
    };

    /// The order a Sleeper draft payload describes, plus a warning when the
    /// type is one this app cannot model (auction) and snake math is used.
    pub fn from_draft(draft: &Draft) -> (DraftOrder, Option<String>) {
        match draft.draft_type.as_str() {
            "snake" => (
                DraftOrder {
                    linear: false,
                    reversal_round: draft.settings.reversal_round.unwrap_or(0),
                },
                None,
            ),
            "linear" => (DraftOrder::LINEAR, None),
            other => (
                DraftOrder::SNAKE,
                Some(format!(
                    "draft type '{other}' is not supported; pick order is modelled as a snake"
                )),
            ),
        }
    }
}

/// Which slot (1-based) is on the clock at a given overall pick (1-based)?
///
/// Total by construction: a malformed or mock draft payload reporting zero
/// teams would otherwise divide by zero and underflow, and `overflow-checks`
/// is on in release, so that is a live-path panic rather than a wrong number.
pub fn slot_for_pick(pick_no: u32, teams: u32, order: DraftOrder) -> u32 {
    if teams == 0 || pick_no == 0 {
        return 1;
    }
    let round = (pick_no - 1) / teams; // 0-based round
    let idx = (pick_no - 1) % teams; // 0-based index within round
    if order.linear {
        return idx + 1;
    }
    let mut forward = round.is_multiple_of(2);
    // Third-round reversal: from that round on, every direction is flipped
    // relative to a plain snake, so the reversal round repeats the previous
    // round's direction and the snake resumes from there.
    if order.reversal_round > 0 && round + 1 >= order.reversal_round {
        forward = !forward;
    }
    if forward {
        idx + 1
    } else {
        teams - idx
    }
}

/// All overall pick numbers (1-based) belonging to a slot.
pub fn picks_for_slot(slot: u32, teams: u32, rounds: u32, order: DraftOrder) -> Vec<u32> {
    (1..=teams * rounds)
        .filter(|&p| slot_for_pick(p, teams, order) == slot)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RosterEntry {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub pick_no: u32,
    pub round: u32,
    /// Kept from last season rather than drafted tonight.
    pub is_keeper: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamRoster {
    pub slot: u32,
    pub display_name: Option<String>,
    pub players: Vec<RosterEntry>,
    /// Open starting slots by label, e.g. {"RB": 1, "FLEX": 2}
    pub open_starters: Vec<(String, u32)>,
}

/// Fill starters greedily in roster_positions order: dedicated slots first,
/// then flex takes leftover eligible players. Returns open (unfilled) slots.
/// P(player still available at overall pick `at_pick`), given their ADP and
/// that they are still on the board at `now_pick`.
///
/// The selection pick is modelled as Normal(adp, sigma). The odds are
/// *conditional* on survival to `now_pick`: `S(at) / S(now)`. Without that, a
/// player 30 picks past his ADP reads 1% for every remaining pick, which says
/// nothing about the players you are actually choosing between. Sigma also
/// widens with the distance already fallen — a market that has been wrong for
/// 50 picks (injury, holdout, stale ADP) is not one to trust to within three.
pub fn survival_probability(adp: f64, now_pick: u32, at_pick: u32) -> f64 {
    if adp <= 0.0 || adp >= 500.0 {
        // No real ADP signal — assume safe.
        return 0.99;
    }
    let now = f64::from(now_pick);
    let at = f64::from(at_pick.max(now_pick));
    let fallen = (now - adp).max(0.0);
    let sigma = (0.22 * adp).max(3.0).max(0.6 * fallen);
    let tail = |pick: f64| crate::scoring::norm_sf((pick - adp) / sigma);
    let now_tail = tail(now).max(1e-12);
    (tail(at) / now_tail).clamp(0.01, 0.99)
}

/// Group picks into per-slot rosters. `slot_of` says whose roster a pick
/// lands on — not `draft_slot`, which is only where the pick *started*: in a
/// league that trades picks the two differ for a quarter of the board.
pub fn build_rosters(
    picks: &[Pick],
    teams: u32,
    rules: &RosterRules,
    slot_names: &std::collections::HashMap<u32, String>,
    keepers: &std::collections::HashSet<u32>,
    slot_of: impl Fn(&Pick) -> u32,
    name_of: impl Fn(&str) -> (String, String, Option<String>),
) -> Vec<TeamRoster> {
    let mut rosters: Vec<TeamRoster> = (1..=teams)
        .map(|slot| TeamRoster {
            slot,
            display_name: slot_names.get(&slot).cloned(),
            players: Vec::new(),
            open_starters: Vec::new(),
        })
        .collect();
    for pick in picks {
        let slot = slot_of(pick);
        if slot == 0 || slot > teams {
            continue;
        }
        let (name, position, team) = name_of(&pick.player_id);
        rosters[(slot - 1) as usize].players.push(RosterEntry {
            player_id: pick.player_id.clone(),
            name,
            position,
            team,
            pick_no: pick.pick_no,
            round: pick.round,
            is_keeper: keepers.contains(&pick.pick_no),
        });
    }
    for roster in &mut rosters {
        roster.open_starters =
            rules.open_starting_slots(roster.players.iter().map(|player| player.position.as_str()));
    }
    rosters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_order_14_teams() {
        let snake = DraftOrder::SNAKE;
        assert_eq!(slot_for_pick(1, 14, snake), 1);
        assert_eq!(slot_for_pick(2, 14, snake), 2);
        assert_eq!(slot_for_pick(14, 14, snake), 14);
        assert_eq!(slot_for_pick(15, 14, snake), 14); // snake turn
        assert_eq!(slot_for_pick(27, 14, snake), 2);
        assert_eq!(slot_for_pick(28, 14, snake), 1);
        assert_eq!(slot_for_pick(29, 14, snake), 1); // next turn
        assert_eq!(slot_for_pick(30, 14, snake), 2);
    }

    #[test]
    fn linear_drafts_keep_the_same_slot_every_round() {
        let linear = DraftOrder::LINEAR;
        assert_eq!(slot_for_pick(15, 14, linear), 1);
        assert_eq!(slot_for_pick(28, 14, linear), 14);
        assert_eq!(picks_for_slot(2, 14, 4, linear), vec![2, 16, 30, 44]);
    }

    #[test]
    fn third_round_reversal_flips_the_order_from_round_three() {
        let order = DraftOrder {
            linear: false,
            reversal_round: 3,
        };
        // Rounds 1–2 as a snake; round 3 repeats round 2's direction; then
        // the snake resumes from there.
        assert_eq!(slot_for_pick(1, 14, order), 1);
        assert_eq!(slot_for_pick(15, 14, order), 14);
        assert_eq!(slot_for_pick(29, 14, order), 14);
        assert_eq!(slot_for_pick(42, 14, order), 1);
        assert_eq!(slot_for_pick(43, 14, order), 1);
        assert_eq!(slot_for_pick(57, 14, order), 14);
        assert_eq!(picks_for_slot(2, 14, 4, order), vec![2, 27, 41, 44]);
    }

    #[test]
    fn the_draft_payload_selects_the_order_and_flags_auctions() {
        let mut draft: Draft = serde_json::from_value(serde_json::json!({
            "draft_id": "d", "status": "pre_draft", "type": "linear",
            "settings": {"teams": 12, "rounds": 15}
        }))
        .unwrap();
        assert_eq!(DraftOrder::from_draft(&draft), (DraftOrder::LINEAR, None));
        draft.draft_type = "snake".into();
        draft.settings.reversal_round = Some(3);
        let (order, warning) = DraftOrder::from_draft(&draft);
        assert_eq!(order.reversal_round, 3);
        assert!(!order.linear);
        assert!(warning.is_none());
        draft.draft_type = "auction".into();
        let (order, warning) = DraftOrder::from_draft(&draft);
        assert_eq!(order, DraftOrder::SNAKE);
        assert!(warning.unwrap().contains("auction"));
    }

    #[test]
    fn slot2_pick_numbers_match_league_doc() {
        // From the spec: slot 2 in a 14-team snake.
        let picks = picks_for_slot(2, 14, 15, DraftOrder::SNAKE);
        assert_eq!(
            picks,
            vec![2, 27, 30, 55, 58, 83, 86, 111, 114, 139, 142, 167, 170, 195, 198]
        );
    }

    #[test]
    fn survival_extremes() {
        // ADP 1 player is nearly gone by pick 27.
        assert!(survival_probability(1.5, 2, 27) < 0.05);
        // ADP 100 player is nearly certain at pick 27.
        assert!(survival_probability(100.0, 2, 27) > 0.95);
    }

    // A player still on the board past his ADP has, by definition, already
    // beaten the unconditional odds. The odds of lasting a few more picks must
    // be judged from where the draft is now, not from pick 1 — otherwise every
    // faller reads 1% and the Surv column says nothing about the players you
    // are actually choosing between.
    #[test]
    fn a_player_already_past_adp_is_not_pinned_at_the_floor() {
        // ADP 20, still here at 27, next pick 30: three picks to survive.
        let p = survival_probability(20.0, 27, 30);
        assert!(p > 0.1, "survival(20 | at 27 -> 30) = {p}");
    }

    #[test]
    fn far_past_adp_players_still_get_a_real_probability() {
        // ADP 4 and still available at 58 (injury, holdout — the market has
        // been wrong for 54 picks). 25 picks to survive: unlikely, not 1%.
        let p = survival_probability(4.0, 58, 83);
        assert!(p > 0.05 && p < 0.5, "survival(4 | at 58 -> 83) = {p}");
    }

    #[test]
    fn conditioning_on_being_available_now_never_lowers_the_odds() {
        for adp in [3.0, 12.0, 20.0, 40.0, 90.0, 150.0] {
            for now in [2u32, 27, 30, 55, 58, 83, 111] {
                let next = now + 25;
                let conditioned = survival_probability(adp, now, next);
                let unconditioned = survival_probability(adp, 1, next);
                assert!(
                    conditioned + 1e-9 >= unconditioned,
                    "adp {adp} now {now}: {conditioned} < {unconditioned}"
                );
            }
        }
    }

    #[test]
    fn survival_over_zero_picks_is_certain() {
        assert!(survival_probability(20.0, 27, 27) >= 0.99);
    }

    #[test]
    fn open_slots_fill_flex_after_dedicated() {
        let roster: Vec<String> = [
            "QB", "RB", "WR", "TE", "FLEX", "FLEX", "FLEX", "FLEX", "DEF", "BN",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let players = [
            RosterEntry {
                player_id: "a".into(),
                name: "A".into(),
                position: "RB".into(),
                team: None,
                pick_no: 2,
                round: 1,
                is_keeper: false,
            },
            RosterEntry {
                player_id: "b".into(),
                name: "B".into(),
                position: "RB".into(),
                team: None,
                pick_no: 27,
                round: 2,
                is_keeper: false,
            },
            RosterEntry {
                player_id: "c".into(),
                name: "C".into(),
                position: "WR".into(),
                team: None,
                pick_no: 30,
                round: 3,
                is_keeper: false,
            },
        ];
        let open = RosterRules::new(&roster)
            .open_starting_slots(players.iter().map(|player| player.position.as_str()));
        // RB and WR dedicated slots filled; 1 RB spills into flex.
        let as_map: std::collections::HashMap<_, _> = open.into_iter().collect();
        assert_eq!(as_map.get("QB"), Some(&1));
        assert_eq!(as_map.get("TE"), Some(&1));
        assert_eq!(as_map.get("DEF"), Some(&1));
        assert_eq!(as_map.get("FLEX"), Some(&3));
        assert_eq!(as_map.get("RB"), None);
        assert_eq!(as_map.get("WR"), None);
    }
}
