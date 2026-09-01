//! The one true view: DraftView is BOTH the UI's data source and the
//! AI-readable state dump — no difference between what human and model see.

use crate::board::AvailablePlayer;
use crate::draft::{self, TeamRoster};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::pick_value::{self, PickPrice};
use crate::recommend::{recommend, Recommendation};
use crate::sleeper::Pick;
use crate::traded_picks::PickOwnership;
use serde::Serialize;
use std::collections::HashMap;

/// The pick list lives in `picks` and the tier scan in `board`; both are
/// re-exported here because callers have always reached for them through the
/// view, which is the one place the whole draft state comes together.
pub use crate::board::tier_alerts;
pub use crate::picks::{keeper_pick_nos, merged_picks, next_open_pick};

#[derive(Debug, Clone, Serialize)]
pub struct DraftStatus {
    pub draft_id: String,
    pub status: String,
    pub teams: u32,
    pub rounds: u32,
    pub pick_timer: Option<u32>,
    pub current_pick: u32,
    pub current_round: u32,
    pub on_clock_slot: u32,
    pub on_clock_name: Option<String>,
    pub my_slot: Option<u32>,
    pub is_my_pick: bool,
    pub picks_until_mine: Option<u32>,
    pub my_next_picks: Vec<u32>,
    pub total_picks_made: usize,
    pub manual_picks_active: bool,
    /// Epoch milliseconds when the current pick's timer expires. Present only
    /// while drafting with a pick timer and a recorded last pick.
    pub clock_deadline_ms: Option<u64>,
    /// Every pick the plain snake gets wrong — because it was traded, or
    /// because the league uses third-round reversal: pick number -> the slot
    /// whose manager makes it. Empty in an ordinary snake league. The
    /// frontend's queue reads this so it never names the wrong manager.
    pub pick_slot_overrides: HashMap<u32, u32>,
    /// Pick numbers held by keepers: already in the book, nobody's turn.
    pub keeper_picks: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierAlert {
    pub position: String,
    pub tier: u32,
    pub players_left: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentPick {
    pub pick_no: u32,
    pub round: u32,
    pub slot: u32,
    pub slot_name: Option<String>,
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
}

/// A position taken `count` times in the last `window` picks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PositionRun {
    pub position: String,
    pub count: u32,
    pub window: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub schema_version: String,
    pub generated_at: u64,
    pub league: LeagueSummary,
    pub draft: DraftStatus,
    pub my_roster: Option<TeamRoster>,
    pub rosters: Vec<TeamRoster>,
    pub available: Vec<AvailablePlayer>,
    pub tier_alerts: Vec<TierAlert>,
    pub position_run: Option<PositionRun>,
    pub recommendations: Vec<Recommendation>,
    pub recent_picks: Vec<RecentPick>,
    pub replacement_baselines: HashMap<String, f64>,
    /// position -> number of league-wide startable players (incl. flex share)
    pub replacement_demand: HashMap<String, usize>,
    /// What a pick in each round of this draft has been worth, in points over
    /// replacement — empty until the draft has picks to learn from.
    pub pick_prices: Vec<PickPrice>,
    pub data_health: DataHealth,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeagueSummary {
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub total_rosters: u32,
    pub roster_positions: Vec<String>,
    pub draftable_positions: Vec<String>,
    pub scoring_settings: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataHealth {
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub board_size: usize,
    pub warnings: Vec<String>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollHealth {
    pub last_success_at: Option<u64>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

pub fn poll_health(loaded: &LoadedLeague) -> PollHealth {
    PollHealth {
        last_success_at: loaded.poll_last_success_at,
        consecutive_failures: loaded.poll_consecutive_failures,
        last_error: loaded.poll_last_error.clone(),
    }
}

fn validated_slot(slot: Option<u32>, teams: u32) -> (Option<u32>, Option<String>) {
    match slot {
        Some(value) if !(1..=teams).contains(&value) => (
            None,
            Some(format!(
                "your draft slot {value} is outside the valid range 1..={teams}"
            )),
        ),
        _ => (slot, None),
    }
}

/// When the current pick's timer runs out, from Sleeper's `last_picked`
/// stamp and the draft's `pick_timer`. Only meaningful mid-draft.
pub fn clock_deadline_ms(
    status: &str,
    last_picked: Option<u64>,
    pick_timer: Option<u32>,
) -> Option<u64> {
    if status != "drafting" {
        return None;
    }
    Some(last_picked? + u64::from(pick_timer.filter(|t| *t > 0)?) * 1000)
}

/// How many recent picks a positional run is judged over, and how many of them
/// have to share a position for it to count as one.
const RUN_WINDOW: u32 = 6;
const RUN_MIN: u32 = 4;

/// The position taken at least `min_count` times in the last `window` picks.
pub fn position_run(positions: &[String], window: u32, min_count: u32) -> Option<PositionRun> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for pos in positions.iter().rev().take(window as usize) {
        if !pos.is_empty() {
            *counts.entry(pos.as_str()).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c >= min_count)
        .max_by_key(|(_, c)| *c)
        .map(|(pos, count)| PositionRun {
            position: pos.to_string(),
            count,
            window,
        })
}

pub fn build_view(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    let league = &loaded.league;
    let draft = &loaded.draft;
    let teams = draft.settings.teams;
    let rounds = draft.settings.rounds;

    let picks = merged_picks(&loaded.api_picks, &loaded.manual_picks);
    let total_picks = picks.len();
    // Where the draft has got to is the first *gap*, not the pick count: a
    // keeper league opens with picks already in the book at 11, 20, 177 …,
    // and counting them puts the clock several rounds ahead of itself.
    let open_pick = next_open_pick(&picks, teams, rounds);
    let current_pick = open_pick.unwrap_or(teams * rounds);
    let draft_over = open_pick.is_none();
    let current_round = (current_pick - 1) / teams + 1;
    let keepers = crate::keepers::known_keepers(loaded, teams, rounds);
    let (order, order_warning) = draft::DraftOrder::from_draft(draft);
    // Who actually picks where: the snake (third-round reversal included),
    // corrected for picks that changed hands.
    let ownership = PickOwnership::from_draft(draft, &loaded.traded_picks, teams, rounds, order);
    let on_clock_slot = ownership.owner_slot(current_pick);

    // Slot display names: draft_order user ids resolved via league users.
    let mut slot_names: HashMap<u32, String> = HashMap::new();
    if let Some(order) = &draft.draft_order {
        for (user_id, slot) in order {
            let name = loaded
                .user_names
                .get(user_id)
                .cloned()
                .unwrap_or_else(|| user_id.clone());
            slot_names.insert(*slot, name);
        }
    }
    let my_slot = config.my_user_id.as_ref().and_then(|uid| {
        draft
            .draft_order
            .as_ref()
            .and_then(|order| order.get(uid).copied())
    });
    // Mock drafts (no league members loaded) may be joined under a guest id:
    // fall back to the draft creator's slot, then to the only joined human.
    // Never applies to a real league, where user_names is populated.
    let my_slot = my_slot.or_else(|| {
        if !loaded.user_names.is_empty() {
            return None;
        }
        let order = draft.draft_order.as_ref()?;
        draft
            .creators
            .as_ref()
            .and_then(|c| c.iter().find_map(|uid| order.get(uid).copied()))
            .or_else(|| {
                if order.len() == 1 {
                    order.values().next().copied()
                } else {
                    None
                }
            })
    });
    let (my_slot, slot_warning) = validated_slot(my_slot, teams);

    // Mine by ownership, not by slot — a pick I traded away is not mine, and
    // one I acquired is. Picks already in the book (my own keepers) are not
    // picks I still get to make.
    let made: std::collections::HashSet<u32> = picks.iter().map(|p| p.pick_no).collect();
    let my_next_picks: Vec<u32> = my_slot
        .map(|slot| {
            ownership
                .picks_owned_by(slot)
                .into_iter()
                .filter(|p| *p >= current_pick && !made.contains(p))
                .collect()
        })
        .unwrap_or_default();
    let is_my_pick = !draft_over && my_slot.is_some() && my_slot == on_clock_slot;
    let picks_until_mine = my_next_picks
        .first()
        .map(|&mine| crate::picks::picks_until(current_pick, mine, &picks));
    // Survival is judged at my next pick AFTER the one I'm making now (or the
    // upcoming one if I'm not on the clock).
    let survival_pick = if is_my_pick {
        my_next_picks.get(1).copied()
    } else {
        my_next_picks.first().copied()
    };

    let taken: std::collections::HashSet<&str> =
        picks.iter().map(|p| p.player_id.as_str()).collect();

    let name_of = |player_id: &str| -> (String, String, Option<String>) {
        if let Some(&i) = loaded.board_index.get(player_id) {
            let p = &loaded.board[i];
            (p.name.clone(), p.position.clone(), p.team.clone())
        } else if let Some(meta) = loaded.player_meta.get(player_id) {
            (
                meta.full_name
                    .clone()
                    .unwrap_or_else(|| player_id.to_string()),
                meta.position.clone().unwrap_or_default(),
                meta.team.clone(),
            )
        } else {
            (player_id.to_string(), String::new(), None)
        }
    };

    // Whose roster a pick lands on: the user who made it, else (manual picks
    // carry no user) whoever owns that pick number.
    let user_slots: HashMap<&str, u32> = draft
        .draft_order
        .iter()
        .flat_map(|o| o.iter().map(|(u, s)| (u.as_str(), *s)))
        .collect();
    let slot_of = |p: &Pick| -> Option<u32> {
        p.picked_by
            .as_deref()
            .and_then(|u| user_slots.get(u).copied())
            .or_else(|| ownership.owner_slot(p.pick_no))
    };
    let rosters = draft::build_rosters(
        &picks,
        teams,
        &loaded.roster_rules,
        &slot_names,
        &keepers,
        slot_of,
        name_of,
    );
    let my_roster = my_slot.and_then(|slot| rosters.get((slot - 1) as usize).cloned());

    // Available players with survival probabilities.
    //
    // Every undrafted player is copied, which is the largest single cost in a
    // poll tick. It stays a copy on purpose: a borrowed `&BoardPlayer` would
    // tie `DraftView` to the lifetime of the `loaded` mutex guard, and every
    // command here builds a view under that guard and then returns it — the
    // view has to outlive the lock. Reserving up front at least keeps the
    // growth from re-copying the vector as it fills.
    let mut available: Vec<AvailablePlayer> = Vec::with_capacity(loaded.board.len());
    available.extend(
        loaded
            .board
            .iter()
            .filter(|p| !taken.contains(p.player_id.as_str()))
            .map(|p| AvailablePlayer {
                survival_next: survival_pick
                    .and_then(|pick| p.adp.map(|adp| draft::survival_probability(adp, pick))),
                player: p.clone(),
            }),
    );

    let tier_alerts = tier_alerts(&available, loaded.roster_rules.draftable_positions());

    // What has actually happened, oldest first. A keeper is in the book but
    // it is not news: it was entered before anyone was on the clock, and a
    // keeper at pick 177 would otherwise be the whole activity feed.
    let happened: Vec<&Pick> = picks
        .iter()
        .filter(|p| p.pick_no < current_pick && !keepers.contains(&p.pick_no))
        .collect();

    // Position run: 4+ of the same position in the last 6 picks. Only those
    // six can be in it, so only those six are looked up.
    let position_run = {
        let recent = &happened[happened.len().saturating_sub(RUN_WINDOW as usize)..];
        let positions: Vec<String> = recent.iter().map(|p| name_of(&p.player_id).1).collect();
        position_run(&positions, RUN_WINDOW, RUN_MIN)
    };

    let recommendations = recommend(
        &available,
        my_roster.as_ref(),
        &loaded.roster_rules,
        current_round,
        rounds,
        current_pick,
    );

    let recent_picks: Vec<RecentPick> = happened
        .iter()
        .rev()
        .take(10)
        .map(|p| {
            let (name, position, team) = name_of(&p.player_id);
            let slot = slot_of(p).unwrap_or(p.draft_slot);
            RecentPick {
                pick_no: p.pick_no,
                round: p.round,
                slot,
                slot_name: slot_names.get(&slot).cloned(),
                player_id: p.player_id.clone(),
                name,
                position,
                team,
            }
        })
        .collect();

    let mut warnings = loaded.warnings.clone();
    warnings.extend(slot_warning);
    warnings.extend(order_warning);
    let mut keeper_picks: Vec<u32> = keepers.iter().copied().collect();
    keeper_picks.sort_unstable();

    DraftView {
        schema_version: "1.1".into(),
        generated_at: now_secs(),
        league: LeagueSummary {
            league_id: league.league_id.clone(),
            name: league.name.clone(),
            season: league.season.clone(),
            total_rosters: league.total_rosters,
            roster_positions: league.roster_positions.clone(),
            draftable_positions: loaded.roster_rules.draftable_positions(),
            scoring_settings: league.scoring_settings.clone(),
        },
        draft: DraftStatus {
            draft_id: draft.draft_id.clone(),
            status: if draft_over {
                "complete".into()
            } else {
                draft.status.clone()
            },
            teams,
            rounds,
            pick_timer: draft.settings.pick_timer,
            current_pick,
            current_round,
            // assemble() refuses a draft with no teams, so this is always Some.
            on_clock_slot: on_clock_slot.unwrap_or(1),
            on_clock_name: on_clock_slot.and_then(|slot| slot_names.get(&slot).cloned()),
            my_slot,
            is_my_pick,
            picks_until_mine,
            my_next_picks,
            total_picks_made: total_picks,
            manual_picks_active: !loaded.manual_picks.is_empty(),
            clock_deadline_ms: clock_deadline_ms(
                &draft.status,
                draft.last_picked,
                draft.settings.pick_timer,
            )
            .filter(|_| !draft_over),
            pick_slot_overrides: ownership.overrides(),
            keeper_picks,
        },
        my_roster,
        rosters,
        available,
        tier_alerts,
        position_run,
        recommendations,
        recent_picks,
        replacement_demand: loaded.replacement_model.demand.clone(),
        pick_prices: pick_value::pick_prices(loaded),
        replacement_baselines: loaded.replacement_model.baseline.clone(),
        data_health: DataHealth {
            players_fetched_at: loaded.players_fetched_at,
            projections_fetched_at: loaded.projections_fetched_at,
            weekly_fetched_at: loaded.weekly_fetched_at,
            board_size: loaded.board.len(),
            warnings,
            poll_last_success_at: loaded.poll_last_success_at,
            poll_consecutive_failures: loaded.poll_consecutive_failures,
            poll_last_error: loaded.poll_last_error.clone(),
        },
    }
}

#[cfg(test)]
mod reliability_tests {
    use super::{clock_deadline_ms, position_run, validated_slot};

    #[test]
    fn invalid_user_slots_are_rejected_before_roster_indexing() {
        assert_eq!(validated_slot(Some(0), 14).0, None);
        assert_eq!(validated_slot(Some(15), 14).0, None);
        assert_eq!(validated_slot(Some(2), 14).0, Some(2));
    }

    #[test]
    fn clock_deadline_is_last_pick_plus_timer_only_while_drafting() {
        assert_eq!(
            clock_deadline_ms("drafting", Some(1_000), Some(90)),
            Some(91_000)
        );
        assert_eq!(clock_deadline_ms("pre_draft", Some(1_000), Some(90)), None);
        assert_eq!(clock_deadline_ms("complete", Some(1_000), Some(90)), None);
        assert_eq!(clock_deadline_ms("drafting", None, Some(90)), None);
        assert_eq!(clock_deadline_ms("drafting", Some(1_000), None), None);
        assert_eq!(clock_deadline_ms("drafting", Some(1_000), Some(0)), None);
    }

    #[test]
    fn position_run_carries_the_count_and_window() {
        let picks: Vec<String> = ["WR", "RB", "RB", "QB", "RB", "RB", "TE"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Last six: RB RB QB RB RB TE -> four RBs.
        let run = position_run(&picks, 6, 4).expect("run");
        assert_eq!((run.position.as_str(), run.count, run.window), ("RB", 4, 6));
        assert_eq!(position_run(&picks, 6, 5), None);
        // Nothing before the window counts: only the first pick is a WR.
        assert_eq!(
            position_run(&picks, 4, 2).map(|r| r.position),
            Some("RB".into())
        );
    }
}
