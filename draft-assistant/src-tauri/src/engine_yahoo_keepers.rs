//! Which Yahoo players are kept, and how that survives to the next poll tick.
//!
//! Yahoo flags a keeper in two places and only one of them is reliable. The
//! `draftresults` resource carries `is_keeper` on some leagues and simply
//! omits it on others — the live resource sends the pick, the round, the team
//! and the player and nothing else — so a keeper league's kept players were
//! drawn as ordinary first-round picks, the board took them off at the wrong
//! moment, and the app's pick maths counted picks nobody was ever going to
//! make. The roster row is the one place Yahoo always answers, as
//! `is_keeper: {"status": .., "cost": .., "kept": "1"}`, and
//! `league/<key>/teams;out=roster` fetches every team's in one call.
//!
//! The roster rows are cached beside the league's other Yahoo files because
//! the poll tick needs them too. A tick re-reads `draftresults` alone;
//! without these on disk every kept pick reverted to "not a keeper" three
//! seconds after the board finished loading. The whole rows are kept rather
//! than the flags alone, because a player who is on a roster is often not in
//! the cached player pool at all — the pool is what is still *available* —
//! and the row is where his name and position are.

use crate::engine::Engine;
use crate::engine_yahoo::cache_name;
use crate::yahoo_types::{YahooDraftPick, YahooPlayer};
use std::collections::HashMap;

/// Yahoo player key -> whether Yahoo says that player is kept.
pub type KeeperFlags = HashMap<String, bool>;

/// The keeper answer for every player on every roster.
///
/// A player Yahoo said nothing about is left out rather than filed as `false`:
/// the absence is what lets the app's own keeper test
/// (`crate::picks::keeper_pick_nos`) have its turn, and reading silence as
/// "not a keeper" is the bug this whole module exists to fix.
pub fn keeper_flags(rosters: &[YahooPlayer]) -> KeeperFlags {
    rosters
        .iter()
        .filter(|player| !player.player_key.is_empty())
        .filter_map(|player| Some((player.player_key.clone(), player.is_keeper?)))
        .collect()
}

/// Fill in the keeper flag on the picks Yahoo left silent about.
///
/// The draft result wins where it exists: it is the record of what happened at
/// the draft, and a player kept last year and drafted normally this year is
/// described correctly there and misleadingly by his roster row.
pub fn apply_keeper_flags(picks: &mut [YahooDraftPick], flags: &KeeperFlags) {
    for pick in picks {
        if pick.is_keeper.is_none() {
            pick.is_keeper = flags.get(&pick.player_key).copied();
        }
    }
}

impl Engine {
    /// Remember this league's rosters for the poll ticks that follow.
    pub async fn save_yahoo_rosters(&self, league_key: &str, rosters: &[YahooPlayer]) {
        self.write_cache_off_thread(&cache_name(league_key, "rosters"), &rosters.to_vec())
            .await;
    }

    /// The rosters the last load wrote down. Never fetches: a tick that went
    /// to Yahoo for these would add a call to every three seconds, and a
    /// roster does not change during a draft except by a pick the tick is
    /// already reading.
    pub async fn yahoo_rosters(&self, league_key: &str) -> Vec<YahooPlayer> {
        self.read_cache_any_off_thread::<Vec<YahooPlayer>>(&cache_name(league_key, "rosters"))
            .await
            .map(|(_, rosters)| rosters)
            .unwrap_or_default()
    }

    /// What a poll tick needs to describe a pick it has just seen: the player
    /// pool the load cached, keyed the way a pick names a player, with the
    /// keeper flags folded onto it.
    ///
    /// Both halves come off disk and neither is fetched. The tick used to pass
    /// an empty map here, so every pick that arrived mid-draft lost its name,
    /// its position and its keeper flag the moment the poller replaced the
    /// load's picks with its own.
    pub async fn yahoo_pick_context(&self, league_key: &str) -> HashMap<String, YahooPlayer> {
        let (pool, rosters) = tokio::join!(
            self.yahoo_cached_pool(league_key),
            self.yahoo_rosters(league_key)
        );
        let mut by_key: HashMap<String, YahooPlayer> = pool
            .into_iter()
            .map(|player| (player.player_key.clone(), player))
            .collect();
        for row in rosters {
            match by_key.get_mut(&row.player_key) {
                // The pool row is the fuller of the two; all the roster adds
                // is the keeper answer.
                Some(player) => player.is_keeper = row.is_keeper.or(player.is_keeper),
                // A player on a roster is not in the pool at all: the pool is
                // what is still available. His row is the only one there is.
                None => {
                    by_key.insert(row.player_key.clone(), row);
                }
            }
        }
        by_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster_row(key: &str, kept: Option<bool>) -> YahooPlayer {
        YahooPlayer {
            player_key: key.to_string(),
            is_keeper: kept,
            ..YahooPlayer::default()
        }
    }

    fn pick(no: u32, player_key: &str, kept: Option<bool>) -> YahooDraftPick {
        YahooDraftPick {
            pick: no,
            round: 1,
            team_key: "449.l.1.t.1".into(),
            player_key: player_key.to_string(),
            cost: None,
            is_keeper: kept,
        }
    }

    /// The failure this prevents: `draftresults` sends no `is_keeper` at all
    /// on most leagues, so every kept player was drawn as an ordinary pick.
    #[test]
    fn a_roster_flag_fills_in_a_pick_the_draft_resource_said_nothing_about() {
        let flags = keeper_flags(&[
            roster_row("449.p.1", Some(true)),
            roster_row("449.p.2", Some(false)),
            roster_row("449.p.3", None),
            roster_row("", Some(true)),
        ]);
        assert_eq!(flags.len(), 2);
        let mut picks = vec![
            pick(1, "449.p.1", None),
            pick(2, "449.p.2", None),
            pick(3, "449.p.3", None),
        ];
        apply_keeper_flags(&mut picks, &flags);
        assert_eq!(picks[0].is_keeper, Some(true));
        assert_eq!(picks[1].is_keeper, Some(false));
        assert_eq!(
            picks[2].is_keeper, None,
            "a player nobody flagged either way must stay unflagged, so the app's own \
             keeper test still gets its turn"
        );
    }

    #[test]
    fn the_draft_resource_wins_where_it_did_answer() {
        let flags = keeper_flags(&[roster_row("449.p.1", Some(true))]);
        let mut picks = vec![pick(1, "449.p.1", Some(false))];
        apply_keeper_flags(&mut picks, &flags);
        assert_eq!(picks[0].is_keeper, Some(false));
    }
}
