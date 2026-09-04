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
/// `None` when the draft has no teams or the pick is before the first one —
/// both would divide by zero or underflow, and neither is worth a panic on
/// data we do not control.
pub fn slot_for_pick(pick_no: u32, teams: u32, order: DraftOrder) -> Option<u32> {
    if teams == 0 || pick_no == 0 {
        return None;
    }
    let round = (pick_no - 1) / teams; // 0-based round
    let idx = (pick_no - 1) % teams; // 0-based index within round
    if order.linear {
        return Some(idx + 1);
    }
    let mut forward = round.is_multiple_of(2);
    // Third-round reversal: from that round on, every direction is flipped
    // relative to a plain snake, so the reversal round repeats the previous
    // round's direction and the snake resumes from there.
    if order.reversal_round > 0 && round + 1 >= order.reversal_round {
        forward = !forward;
    }
    Some(if forward { idx + 1 } else { teams - idx })
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

/// P(player still available at market position `at_pick`), given their ADP.
/// Selection pick modeled as Normal(adp, sigma).
///
/// Sigma is fitted against this app's own completed draft: the standard
/// deviation of (pick made - ADP) sits around 20-40 picks across the board,
/// and is flat-to-slightly-larger at the top rather than tiny there. The old
/// `0.22 * adp` floored at 3 said the opposite - 3 picks of spread at the top
/// of round one and 51 at the end of round fifteen - which made every early
/// name read as certainly gone and every late one as a coin flip. The linear
/// term is kept because the spread does widen, but the clamp is what carries
/// the fit.
///
/// `at_pick` is a *market* position, not an overall pick number: ADP counts
/// selections, so anything that is not a selection must not advance it. Pass
/// `market_pick` of the overall pick, never the overall pick itself — in a
/// league with no keepers the two are the same number.
pub fn survival_probability(adp: f64, at_pick: u32) -> f64 {
    survival_probability_in(adp, at_pick, TWELVE_TEAM)
}

/// The league size the spread above was fitted on.
pub const TWELVE_TEAM: u32 = 12;

/// `survival_probability` in a league that is not twelve teams.
///
/// The 18-35 pick clamp is a *round* count wearing pick clothing: a pick and a
/// half at the bottom and just under three rounds at the top, of a twelve-team
/// round. In a ten-team league those same numbers are wider in rounds than
/// they were fitted to be and every player reads as likelier to last; in a
/// fourteen they are narrower and the board reads as picked over. Scaled by
/// the real league size, the band means the same thing everywhere, and it is
/// rounds, not raw pick counts, that a drafter waits through.
pub fn survival_probability_in(adp: f64, at_pick: u32, teams: u32) -> f64 {
    if adp <= 0.0 || adp >= 500.0 {
        // No real ADP signal — assume safe.
        return 0.99;
    }
    let size = teams.max(1) as f64 / TWELVE_TEAM as f64;
    let sigma = (0.35 * adp).clamp((18.0 * size).round(), (35.0 * size).round());
    let z = (at_pick as f64 - adp) / sigma;
    (1.0 - crate::scoring::norm_cdf(z)).clamp(0.01, 0.99)
}

/// Where an overall pick number sits in the *market* an ADP is measured in:
/// how many selections will have been made by the time it arrives.
///
/// Keepers are entered as picks hours before anybody is on the clock, and
/// nobody ever selects at those numbers — a keeper league's overall pick 27
/// can be the fourth player actually chosen. Measuring an ADP against 27 there
/// says a first-rounder is long gone before a single name has been called, and
/// every survival percentage, the "Won't last" rail and the survival lines on
/// the recommendation cards were pessimistic all night because of it.
///
/// With no keepers this is the identity: the market and the board agree.
pub fn market_pick(at_pick: u32, keepers: &std::collections::HashSet<u32>) -> u32 {
    let ahead = keepers.iter().filter(|&&k| k < at_pick).count() as u32;
    at_pick.saturating_sub(ahead).max(1)
}

/// Group picks into per-slot rosters.
///
/// `slot_of` says whose roster a pick lands on — not `draft_slot`, which is
/// only where the pick *started*: in a league that trades picks the two
/// differ for a good fraction of the board.
pub fn build_rosters(
    picks: &[Pick],
    teams: u32,
    rules: &RosterRules,
    slot_names: &std::collections::HashMap<u32, String>,
    keepers: &std::collections::HashSet<u32>,
    slot_of: impl Fn(&Pick) -> Option<u32>,
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
        let Some(slot) = slot_of(pick) else { continue };
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
    use crate::traded_picks::PickOwnership;
    use std::collections::HashSet;

    /// The pick numbers a slot makes, read off the live path the app uses.
    /// `PickOwnership::plain` is the same snake with no trades applied.
    fn picks_for_slot(slot: u32, teams: u32, rounds: u32, order: DraftOrder) -> Vec<u32> {
        PickOwnership::plain(teams, rounds, order).picks_owned_by(slot)
    }

    #[test]
    fn snake_order_14_teams() {
        let snake = DraftOrder::SNAKE;
        assert_eq!(slot_for_pick(1, 14, snake), Some(1));
        assert_eq!(slot_for_pick(2, 14, snake), Some(2));
        assert_eq!(slot_for_pick(14, 14, snake), Some(14));
        assert_eq!(slot_for_pick(15, 14, snake), Some(14)); // snake turn
        assert_eq!(slot_for_pick(27, 14, snake), Some(2));
        assert_eq!(slot_for_pick(28, 14, snake), Some(1));
        assert_eq!(slot_for_pick(29, 14, snake), Some(1)); // next turn
        assert_eq!(slot_for_pick(30, 14, snake), Some(2));
    }

    #[test]
    fn linear_drafts_keep_the_same_slot_every_round() {
        let linear = DraftOrder::LINEAR;
        assert_eq!(slot_for_pick(15, 14, linear), Some(1));
        assert_eq!(slot_for_pick(28, 14, linear), Some(14));
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
        assert_eq!(slot_for_pick(1, 14, order), Some(1));
        assert_eq!(slot_for_pick(15, 14, order), Some(14));
        assert_eq!(slot_for_pick(29, 14, order), Some(14));
        assert_eq!(slot_for_pick(42, 14, order), Some(1));
        assert_eq!(slot_for_pick(43, 14, order), Some(1));
        assert_eq!(slot_for_pick(57, 14, order), Some(14));
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
    fn pick_math_is_total_on_a_draft_that_reports_nothing() {
        // Sleeper has handed us `teams: 0` before; dividing by it used to
        // panic the whole command task on every view build.
        assert_eq!(slot_for_pick(1, 0, DraftOrder::SNAKE), None);
        assert_eq!(slot_for_pick(0, 14, DraftOrder::SNAKE), None);
        assert!(picks_for_slot(1, 0, 15, DraftOrder::SNAKE).is_empty());
        assert!(picks_for_slot(1, 14, 0, DraftOrder::SNAKE).is_empty());
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
        // An ADP 1 player is probably gone by pick 27; an ADP 100 player is
        // all but certain to still be there. The gap between them, not either
        // number alone, is what the survival rail is read for.
        assert!(survival_probability(1.5, 27) < 0.15);
        assert!(survival_probability(100.0, 27) > 0.95);
        assert!(survival_probability(100.0, 27) - survival_probability(1.5, 27) > 0.8);
    }

    #[test]
    fn the_spread_is_the_fitted_band_at_every_depth() {
        // The fit this app's own completed draft gives: sd(pick - ADP) is
        // roughly 20-40 picks, and does not collapse at the top of the board.
        // Read off the curve as where survival crosses one sigma (16%), which
        // must land that far past ADP for a first-rounder and a last-rounder
        // alike. The old 0.22*adp rule put it three picks past ADP at the top.
        for adp in [3.0, 12.0, 60.0, 180.0] {
            let one_sigma = (1..500)
                .find(|&pick| survival_probability(adp, pick) < 0.159)
                .expect("the curve crosses 16% somewhere") as f64;
            assert!(
                (18.0..=36.0).contains(&(one_sigma - adp)),
                "adp {adp} crosses 16% at pick {one_sigma}"
            );
        }
    }

    #[test]
    fn the_spread_is_measured_in_rounds_not_in_twelve_team_picks() {
        // The fitted band is 18-35 picks of a twelve-team round. Held fixed,
        // it is nearly three rounds of a ten-team league at the top and barely
        // two of a fourteen — the same clamp saying two different things.
        // Scaled, the one-sigma point sits the same number of *rounds* past
        // ADP whatever the league size is.
        let rounds_to_one_sigma = |adp: f64, teams: u32| {
            let pick = (1..500)
                .find(|&pick| survival_probability_in(adp, pick, teams) < 0.159)
                .expect("the curve crosses 16% somewhere") as f64;
            (pick - adp) / teams as f64
        };
        for adp in [3.0, 12.0, 180.0] {
            let ten = rounds_to_one_sigma(adp, 10);
            let fourteen = rounds_to_one_sigma(adp, 14);
            assert!(
                (ten - fourteen).abs() < 0.2,
                "adp {adp}: {ten} rounds at ten teams vs {fourteen} at fourteen"
            );
        }
        // Twelve teams is exactly what it always was.
        assert_eq!(
            survival_probability_in(20.0, 27, 12),
            survival_probability(20.0, 27)
        );
        // In picks, the band moves the other way, and that is the point: a
        // ten-team round is ten picks, so the same round and a half of spread
        // is fifteen picks rather than eighteen, and pick 30 is already deep
        // into round three there.
        assert!(survival_probability_in(12.0, 30, 10) < survival_probability_in(12.0, 30, 14));
    }

    #[test]
    fn keepers_in_front_of_a_pick_do_not_advance_the_market() {
        // Twenty-three of the twenty-six picks before 27 are keepers, so only
        // three players have actually been chosen when 27 arrives.
        let keepers: HashSet<u32> = (1..=23).collect();
        assert_eq!(market_pick(27, &keepers), 4);
        // Keepers behind the pick have already been counted out of it, and
        // keepers beyond it are somebody else's problem.
        let later: HashSet<u32> = [40, 55].into_iter().collect();
        assert_eq!(market_pick(27, &later), 27);
        // No keepers at all: the market and the board are the same number.
        assert_eq!(market_pick(27, &HashSet::new()), 27);
        // A pick whose every predecessor is kept is still the first selection.
        assert_eq!(market_pick(5, &(1..=4).collect()), 1);
    }

    #[test]
    fn a_keeper_heavy_book_makes_the_same_player_likelier_to_last() {
        let keepers: HashSet<u32> = (1..=20).collect();
        let clean = survival_probability(20.0, market_pick(27, &HashSet::new()));
        let kept = survival_probability(20.0, market_pick(27, &keepers));
        assert!(clean < 0.4, "pessimistic without keepers: {clean}");
        assert!(kept > 0.7, "seven real picks away: {kept}");
        assert!(kept - clean > 0.3, "{kept} vs {clean}");
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
