//! Yahoo shapes onto the app's existing Sleeper-shaped ones.
//!
//! Everything above the platform clients — the board, the recommender, the
//! chat context — is written against [`crate::sleeper`]'s `League`, `Pick` and
//! `PlayerMeta`. Rather than teach all of that about a second platform, a
//! Yahoo league is translated into those same shapes here. Pure functions
//! only: no clock, no network, no config.
//!
//! Where Yahoo has nothing to say the field is defaulted, and every one of
//! those defaults is called out in a comment at the site — `draft_id`,
//! `previous_league_id`, the playoff knobs, `years_exp`, `age`.

use crate::sleeper::{League, LeagueSettings, Pick, PickMeta, PlayerMeta};
use crate::yahoo_types::{RosterSlot, StatModifier, YahooDraftPick, YahooLeague, YahooTeam};
use std::collections::HashMap;

/// The Sleeper scoring keys each Yahoo stat id pays out to.
///
/// Yahoo identifies a scoring rule by a numeric stat id; Sleeper (and so this
/// app's scoring engine, `crate::scoring`) identifies one by name. This is the
/// whole crosswalk for the standard NFL ids. A few Yahoo ids cover what
/// Sleeper splits in three — id 16 is "2-point conversions" however they were
/// scored — so the value is a list, and the modifier is applied to each.
///
/// Ids not listed here (bonus categories, IDP, Yahoo's own composite stats,
/// and return yardage, which Yahoo counts as one number and Sleeper splits
/// between kick and punt returns) are dropped: a wrong guess at a key would
/// add a wrong number to every player. Dropping one is not silent, though —
/// [`unscored_stats_warning`] puts the ones a league actually pays for on the
/// board's health strip.
pub const YAHOO_STAT_IDS: &[(u32, &[&str])] = &[
    // Offence
    (4, &["pass_yd"]),
    (5, &["pass_td"]),
    (6, &["pass_int"]),
    (9, &["rush_yd"]),
    (10, &["rush_td"]),
    (11, &["rec"]),
    (12, &["rec_yd"]),
    (13, &["rec_td"]),
    (15, &["st_td"]),
    (16, &["pass_2pt", "rush_2pt", "rec_2pt"]),
    (17, &["fum"]),
    (18, &["fum_lost"]),
    // Kicking
    (19, &["fgm_0_19"]),
    (20, &["fgm_20_29"]),
    (21, &["fgm_30_39"]),
    (22, &["fgm_40_49"]),
    (23, &["fgm_50p"]),
    (25, &["fgmiss_0_19"]),
    (26, &["fgmiss_20_29"]),
    (27, &["fgmiss_30_39"]),
    (28, &["fgmiss_40_49"]),
    (29, &["xpm"]),
    (30, &["xpmiss"]),
    // Team defence
    (32, &["sack"]),
    (33, &["int"]),
    (34, &["fum_rec"]),
    (35, &["def_td"]),
    (36, &["safe"]),
    (37, &["blk_kick"]),
    (50, &["pts_allow_0"]),
    (51, &["pts_allow_1_6"]),
    (52, &["pts_allow_7_13"]),
    (53, &["pts_allow_14_20"]),
    (54, &["pts_allow_21_27"]),
    (55, &["pts_allow_28_34"]),
    (56, &["pts_allow_35p"]),
];

/// Yahoo's roster slot names to the app's.
///
/// The flex slots are the whole reason this exists: Yahoo writes a flex as the
/// positions it accepts (`W/R/T`), Sleeper names it (`FLEX`). Everything else
/// — `QB`, `RB`, `WR`, `TE`, `K`, `DEF`, `BN`, `IR` — is already the same word
/// and passes through uppercased.
pub fn roster_position(yahoo: &str) -> String {
    match yahoo.trim().to_ascii_uppercase().as_str() {
        "W/R/T" => "FLEX".to_string(),
        "Q/W/R/T" | "W/R/T/Q" => "SUPER_FLEX".to_string(),
        "W/R" => "WRRB_FLEX".to_string(),
        "W/T" => "REC_FLEX".to_string(),
        "D" | "DST" | "D/ST" => "DEF".to_string(),
        other => other.to_string(),
    }
}

/// The slot list Sleeper would have given: one entry per seat.
pub fn roster_positions(slots: &[RosterSlot]) -> Vec<String> {
    slots
        .iter()
        .flat_map(|slot| {
            let name = roster_position(&slot.position);
            vec![name; slot.count as usize]
        })
        .collect()
}

/// Yahoo's `draft_status` in the app's vocabulary.
///
/// `in_season` is what the app calls a league whose draft is done, which is
/// also what Sleeper calls it; nothing downstream distinguishes "drafted but
/// not started" from "week 3".
pub fn league_status(draft_status: &str) -> String {
    match draft_status.trim().to_ascii_lowercase().as_str() {
        "predraft" => "pre_draft",
        "draft" | "drafting" => "drafting",
        "postdraft" => "in_season",
        // Yahoo has added statuses before; an unknown one is safest read as
        // "not drafting", which only costs the live board.
        _ => "pre_draft",
    }
    .to_string()
}

/// The league's scoring rules as `crate::scoring` wants them.
pub fn scoring_settings(modifiers: &[StatModifier]) -> HashMap<String, f64> {
    let table: HashMap<u32, &[&str]> = YAHOO_STAT_IDS.iter().copied().collect();
    let mut scoring = HashMap::new();
    for modifier in modifiers {
        let Some(keys) = table.get(&modifier.stat_id) else {
            continue;
        };
        for key in *keys {
            scoring.insert((*key).to_string(), modifier.value);
        }
    }
    scoring
}

/// The scoring rules this league pays for that [`YAHOO_STAT_IDS`] cannot
/// translate, named the way Yahoo names them.
///
/// A rule worth zero is left out: a league that scores a category at 0.0 loses
/// nothing by the app not knowing it, and Yahoo lists plenty of those.
pub fn unscored_stats(league: &YahooLeague) -> Vec<String> {
    let known: HashMap<u32, &[&str]> = YAHOO_STAT_IDS.iter().copied().collect();
    let names: HashMap<u32, &str> = league
        .stat_categories
        .iter()
        .map(|category| (category.stat_id, category.name.as_str()))
        .collect();
    league
        .stat_modifiers
        .iter()
        .filter(|modifier| modifier.value != 0.0 && !known.contains_key(&modifier.stat_id))
        .map(|modifier| match names.get(&modifier.stat_id) {
            Some(name) => format!("{name} ({})", modifier.stat_id),
            // Yahoo sends `stat_categories` only when the settings were asked
            // for; without it the id is all there is to report, and an id is
            // still enough to look up.
            None => format!("Yahoo stat {}", modifier.stat_id),
        })
        .collect()
}

/// [`unscored_stats`] as one line for the board's warnings, or nothing when
/// every rule the league pays for is understood.
pub fn unscored_stats_warning(league: &YahooLeague) -> Option<String> {
    let missing = unscored_stats(league);
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "this league scores {} the app cannot read from the projections, so {} worth nothing here: {}",
        if missing.len() == 1 { "a category" } else { "categories" },
        if missing.len() == 1 { "it is" } else { "they are" },
        missing.join(", ")
    ))
}

/// A Yahoo league as the app's `League`.
pub fn league(yahoo: &YahooLeague) -> League {
    League {
        // The Yahoo key (`449.l.12345`) is the id every Yahoo call takes, and
        // it cannot be confused with a Sleeper id, which is all digits.
        league_id: yahoo.league_key.clone(),
        name: yahoo.name.clone(),
        season: yahoo.season.clone(),
        status: league_status(&yahoo.draft_status),
        total_rosters: yahoo.num_teams,
        roster_positions: roster_positions(&yahoo.roster_positions),
        scoring_settings: scoring_settings(&yahoo.stat_modifiers),
        // Yahoo has no separate draft resource: the draft is addressed by the
        // league key, so there is no id to carry here.
        draft_id: None,
        // Yahoo does expose last season's league through `renew`, but nothing
        // in this lane reads it; the "Last season" tab stays Sleeper-only
        // until someone wires it.
        previous_league_id: None,
        // The playoff and waiver knobs live in Yahoo's settings payload but
        // are only used by the season screen, which is not part of the draft
        // deliverable. Defaulted, deliberately.
        settings: LeagueSettings::default(),
    }
}

/// The draft slot each team key drafts from.
pub fn draft_slots(teams: &[YahooTeam]) -> HashMap<String, u32> {
    teams
        .iter()
        .filter_map(|team| Some((team.team_key.clone(), team.draft_position?)))
        .collect()
}

/// The app's player id for a Yahoo player.
///
/// Prefixed so a Yahoo id can never collide with a Sleeper one and so the
/// crosswalk lane 2 builds can tell at a glance which side a row came from.
pub fn player_id(player_key: &str) -> String {
    let bare = player_key.rsplit('.').next().unwrap_or(player_key);
    format!("yahoo:{bare}")
}

/// Yahoo draft results as the app's picks.
///
/// `draft_slot` is the team's `draft_position` — the app's roster id for a
/// draft — so a team with no draft position yet (Yahoo leaves it unset until
/// the order is drawn) lands in slot 0 and is treated as unknown rather than
/// silently attributed to team 1. Picks Yahoo has recorded but not filled
/// (an empty `player_key`) are dropped: they are not picks yet.
///
/// Auction `cost` has nowhere to live on a `Pick`; [`auction_costs`] returns
/// it separately for whoever wants to show it.
pub fn picks(
    results: &[YahooDraftPick],
    teams: &[YahooTeam],
    players: &HashMap<String, crate::yahoo_types::YahooPlayer>,
) -> Vec<Pick> {
    let slots = draft_slots(teams);
    results
        .iter()
        .filter(|result| !result.player_key.is_empty())
        .map(|result| Pick {
            round: result.round,
            pick_no: result.pick,
            draft_slot: slots.get(&result.team_key).copied().unwrap_or(0),
            player_id: player_id(&result.player_key),
            picked_by: Some(result.team_key.clone()),
            metadata: players.get(&result.player_key).map(|player| PickMeta {
                first_name: Some(player.first.clone()),
                last_name: Some(player.last.clone()),
                position: Some(player.display_position.clone()),
                team: Some(player.editorial_team_abbr.to_ascii_uppercase()),
            }),
            // Yahoo marks keepers in its own settings, not on the pick. The
            // app's real keeper test (`crate::picks::keeper_pick_nos`) works
            // off pick numbers anyway, so `None` costs nothing.
            is_keeper: None,
        })
        .collect()
}

/// What each drafted player went for, by app player id. Empty for a snake
/// draft, where Yahoo sends no `cost` at all.
pub fn auction_costs(results: &[YahooDraftPick]) -> HashMap<String, f64> {
    results
        .iter()
        .filter(|result| !result.player_key.is_empty())
        .filter_map(|result| Some((player_id(&result.player_key), result.cost?)))
        .collect()
}

/// A Yahoo player as the app's player row, plus the two facts that have no
/// slot on `PlayerMeta`.
///
/// No `PartialEq`: `crate::sleeper::PlayerMeta` does not derive one, and this
/// lane does not edit `sleeper.rs`. Tests compare the fields they care about.
#[derive(Debug, Clone)]
pub struct MappedPlayer {
    /// `yahoo:30977`.
    pub id: String,
    pub meta: PlayerMeta,
    /// Yahoo knows the bye week; `PlayerMeta` has no field for it. Kept here
    /// so the season lane can use it without another call.
    pub bye_week: Option<u32>,
    /// The Yahoo key the pick list refers to (`449.p.30977`).
    pub player_key: String,
}

impl MappedPlayer {
    /// The key lane 2's crosswalk matches Sleeper rows on: normalised name,
    /// team, position — the same normalisers the projections import uses, so
    /// both sides of the match are spelled the same way.
    pub fn crosswalk_key(&self) -> (String, String, String) {
        (
            crate::second_opinion::normalize_name(self.meta.full_name.as_deref().unwrap_or("")),
            self.meta.team.clone().unwrap_or_default(),
            crate::second_opinion::normalize_position(self.meta.position.as_deref().unwrap_or("")),
        )
    }
}

/// One Yahoo player as a `PlayerMeta` row.
pub fn player(yahoo: &crate::yahoo_types::YahooPlayer) -> MappedPlayer {
    let position = crate::second_opinion::normalize_position(&yahoo.display_position);
    MappedPlayer {
        id: player_id(&yahoo.player_key),
        meta: PlayerMeta {
            full_name: Some(yahoo.full_name.clone()),
            first_name: Some(yahoo.first.clone()),
            last_name: Some(yahoo.last.clone()),
            position: Some(position),
            // Yahoo writes team abbreviations mixed case ("Cin"); every other
            // source in this app writes them upper.
            team: Some(yahoo.editorial_team_abbr.to_ascii_uppercase()),
            fantasy_positions: Some(
                yahoo
                    .eligible_positions
                    .iter()
                    .map(|position| crate::second_opinion::normalize_position(position))
                    .filter(|position| position != "IR" && !position.contains('/'))
                    .collect(),
            ),
            // Yahoo's `status` is already the one-letter code the app shows
            // ("Q", "O", "IR"); a healthy player has none.
            injury_status: yahoo.status.clone(),
            // Yahoo exposes neither of these on the player resource. Nothing
            // in the draft board needs them; the rookie badge stays Sleeper's.
            years_exp: None,
            age: None,
        },
        bye_week: yahoo.bye_week,
        player_key: yahoo.player_key.clone(),
    }
}

/// Every player in a page, mapped.
pub fn players(rows: &[crate::yahoo_types::YahooPlayer]) -> Vec<MappedPlayer> {
    rows.iter().map(player).collect()
}

#[cfg(test)]
#[path = "yahoo_map_tests.rs"]
mod tests;
