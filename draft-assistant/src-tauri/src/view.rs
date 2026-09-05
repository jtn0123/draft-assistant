//! The one true view: DraftView is BOTH the UI's data source and the
//! AI-readable state dump — no difference between what human and model see.

use crate::board::AvailablePlayer;
use crate::draft::{self};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::pick_value;
use crate::recommend::{recommend, RecommendInputs};
use crate::sleeper::Pick;
use crate::traded_picks::PickOwnership;
use crate::view_signals::{clock_deadline_ms, validated_slot, RUN_MIN, RUN_WINDOW};
use std::collections::HashMap;

/// The pick list lives in `picks` and the tier scan in `board`; both are
/// re-exported here because callers have always reached for them through the
/// view, which is the one place the whole draft state comes together. The
/// view's own shapes and derived signals sit in `view_types` and
/// `view_signals`, and are re-exported for the same reason.
pub use crate::board::tier_alerts;
pub use crate::picks::{keeper_pick_nos, merged_picks, next_open_pick};
pub use crate::view_signals::position_run;
pub use crate::view_types::{
    DataHealth, DraftStatus, DraftView, LeagueSummary, PositionRun, RecentPick, TierAlert,
    DRAFT_SCHEMA_VERSION,
};

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
    // Yahoo names the logged-in user's own team on the team resource, so the
    // loader has already worked the slot out; there is no Sleeper user id to
    // look it up with.
    let my_slot = my_slot.or(loaded.my_slot);
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
    // Keepers are handed in because a pick already in the book is nobody's
    // turn: two of my picks with only keepers between them are one window.
    let survival_pick =
        crate::view_signals::survival_target(&my_next_picks, current_pick, is_my_pick, &keepers);
    // …and judged against the market, not the board. A keeper sitting between
    // here and that pick is already in the book: nobody selects at its number,
    // so it must not age the ADP the survival is read off. With 27 keepers the
    // unadjusted number said a first-rounder was gone before a name had been
    // called. Identical to `survival_pick` in a league with no keepers.
    let survival_market_pick = survival_pick.map(|pick| draft::market_pick(pick, &keepers));

    let taken: std::collections::HashSet<&str> =
        picks.iter().map(|p| p.player_id.as_str()).collect();

    // Board first, then Sleeper's player dictionary — the season screen's
    // `Lookup`, rather than a second copy of it. The copy this replaces had no
    // first-name/last-name fallback, so a player the dictionary spells only in
    // parts (plenty of defences and rookies) rendered on the draft side as a
    // raw numeric id.
    let lookup = crate::season_lookup::Lookup { loaded };
    let name_of = |player_id: &str| -> (String, String, Option<String>) {
        (
            lookup.name(player_id),
            lookup.position(player_id).unwrap_or_default(),
            lookup.team(player_id),
        )
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
                survival_next: survival_market_pick.and_then(|pick| {
                    p.adp
                        .map(|adp| draft::survival_probability_in(adp, pick, teams))
                }),
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

    // Byes already stacked on my starters: which week, and how many of them.
    // Only the recommender can price this, and only the view can look a
    // rostered player's bye week up on the board. Which of them start is
    // `starter_byes`' problem — the count used to be of the whole roster,
    // under a reason line that said "starters".
    let my_bye_roster: Vec<(&str, Option<u32>)> = my_roster
        .iter()
        .flat_map(|roster| roster.players.iter())
        // A kicker's or a defence's bye is a waiver-wire errand, not a
        // lineup problem, so neither is worth pricing a candidate over.
        .filter(|player| !crate::board::is_late_only(&player.position))
        .map(|player| {
            let bye = loaded
                .board_index
                .get(&player.player_id)
                .and_then(|i| loaded.board.get(*i))
                .and_then(|p| p.bye_week);
            (player.position.as_str(), bye)
        })
        .collect();
    let my_byes = crate::view_signals::starter_byes(&loaded.roster_rules, my_bye_roster);
    let recommendations = recommend(&RecommendInputs {
        available: &available,
        my_roster: my_roster.as_ref(),
        rules: &loaded.roster_rules,
        current_round,
        total_rounds: rounds,
        current_pick,
        market_pick: draft::market_pick(current_pick, &keepers),
        teams,
        points_per_reception: league.scoring_settings.get("rec").copied().unwrap_or(0.0),
        position_run: position_run.as_ref(),
        my_byes: &my_byes,
        pre_draft: draft.status == "pre_draft",
    });

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
        schema_version: DRAFT_SCHEMA_VERSION.into(),
        generated_at: now_secs(),
        league: LeagueSummary {
            league_id: league.league_id.clone(),
            platform: crate::view_types::platform_for(&league.league_id).to_string(),
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
            // A paused draft is not on anybody's clock: Sleeper stops the
            // timer, and the screen used to keep counting down and keep
            // saying whose turn it was as if nothing had happened.
            paused: !draft_over && draft.status == "paused",
            clock_deadline_ms: clock_deadline_ms(
                &draft.status,
                draft.last_picked,
                draft.settings.pick_timer,
                draft.start_time,
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
            second_opinion_loaded_at: loaded.second_opinion_loaded_at,
        },
    }
}
