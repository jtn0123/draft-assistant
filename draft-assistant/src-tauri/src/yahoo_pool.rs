//! Walking Yahoo's player pool a page at a time, and keeping what arrived.
//!
//! The pool is the expensive call: 25 players a page, so a real league runs to
//! a couple of dozen requests, and Yahoo starts throttling somewhere in the
//! middle of that. Two things used to go wrong there.
//!
//! The walk stopped early. It asked "did this page hand back fewer rows than I
//! asked for?" of the *filtered* rows, so one player Yahoo sent without a
//! `player_key` — a free-agent row mid-rebuild, say — ended the pool at page
//! four with three hundred players missing from the board. The page's own
//! `count` is what Yahoo says it sent, and that is what the walk reads now.
//!
//! And a throttle threw the work away. A 999 on page eleven failed the whole
//! call, the ten pages already in hand were dropped, and the retry started at
//! page zero — straight back into the throttle. A [`PlayerPool`] carries the
//! pages that did arrive plus where to pick up, so the retry resumes.

use crate::yahoo::{YahooClient, YahooError, PAGE};
use crate::yahoo_types::YahooPlayer;
use serde::{Deserialize, Serialize};

/// The player pool as far as it has been read.
///
/// Cached in this shape rather than as a bare list so that an interrupted
/// load is stored as what it is. `complete: false` is the marker that says
/// "there is more of this to fetch", and it is why a partial pool can be kept
/// on disk without a later load mistaking it for the whole league.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerPool {
    pub players: Vec<YahooPlayer>,
    /// The `start` the next page should ask for. Tracked rather than derived
    /// from `players.len()`, which drifts the moment Yahoo sends a row this
    /// app cannot read.
    pub next_start: u32,
    /// Whether Yahoo has run out of players to hand over.
    pub complete: bool,
}

impl PlayerPool {
    /// A pool that has nothing in it yet.
    pub fn empty() -> Self {
        Self::default()
    }
}

impl YahooClient {
    /// Every player Yahoo will hand over, one page at a time.
    ///
    /// Yahoo reports no total, so the end is a page that comes back shorter
    /// than it was asked for. `limit` is a stop of last resort: without it a
    /// server that kept answering with full pages would page forever.
    pub async fn all_players(
        &self,
        league_key: &str,
        position: Option<&str>,
        limit: u32,
    ) -> Result<Vec<YahooPlayer>, YahooError> {
        let (pool, error) = self
            .pool_from(league_key, position, limit, PlayerPool::empty())
            .await;
        match error {
            Some(error) => Err(error),
            None => Ok(pool.players),
        }
    }

    /// Carry on reading the pool from where `have` left off.
    ///
    /// Never fails outright: it hands back everything it managed to read plus
    /// the error that stopped it, so the caller can keep the pages that did
    /// arrive. A pool that comes back with no error is complete.
    pub async fn pool_from(
        &self,
        league_key: &str,
        position: Option<&str>,
        limit: u32,
        have: PlayerPool,
    ) -> (PlayerPool, Option<YahooError>) {
        let mut pool = have;
        pool.complete = false;
        while pool.next_start < limit {
            let page = match self
                .players(league_key, pool.next_start, PAGE, position)
                .await
            {
                Ok(page) => page,
                Err(error) => return (pool, Some(error)),
            };
            // `count` is the rows Yahoo sent; `players` is the rows this app
            // could read. Paging on the second one ends the walk on the first
            // row Yahoo sends in a shape the parser drops.
            let sent = page.count as u32;
            pool.players.extend(page.players);
            pool.next_start += PAGE;
            if sent < PAGE {
                pool.complete = true;
                break;
            }
        }
        // Stopping at the ceiling counts as finished: there is no more of the
        // pool this app will ever ask for, and leaving it marked incomplete
        // would send every later load back to Yahoo for pages it will not
        // read.
        pool.complete |= pool.next_start >= limit;
        (pool, None)
    }
}

#[cfg(test)]
#[path = "yahoo_pool_tests.rs"]
mod tests;
