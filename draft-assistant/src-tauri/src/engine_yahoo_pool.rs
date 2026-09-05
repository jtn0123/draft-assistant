//! The cached, resumable read of a Yahoo league's player pool.
//!
//! Separate from `engine_yahoo.rs` because the pool is the one Yahoo resource
//! that is not a single request: it is two dozen of them, and the rules for
//! what to keep when the twelfth fails are the whole point of this file.
//!
//! The policy, in order:
//!
//! 1. A complete, fresh cache is served without a request.
//! 2. Otherwise every page that arrives is kept, and the cache is written
//!    whether or not the walk finished. A partial pool is stored as one
//!    (`complete: false`), so the next attempt resumes at the page the
//!    throttle stopped on instead of at page zero — but only while it is
//!    fresh. Yahoo's pool is not a fixed list: players are added, dropped and
//!    re-ordered, and page 12 of an hour-old walk is not page 12 of this
//!    one. A stale partial is started again from the top rather than stitched
//!    onto a pool that has moved underneath it.
//! 3. A walk that fails having read nothing new falls back to whatever was on
//!    disk, with a warning, exactly as the other Yahoo reads do.
//! 4. Only a failure with nothing behind it at all takes the load down.

use crate::engine::{Engine, PLAYERS_TTL_SECS};
use crate::engine_yahoo::cache_name;
use crate::yahoo::{YahooClient, PAGE};
use crate::yahoo_pool::PlayerPool;
use crate::yahoo_types::YahooPlayer;

/// Yahoo's pages are 25 players each, so the pool is the most expensive thing
/// in a load. This is the stop of last resort; a real league's pool ends
/// first.
pub(crate) const POOL_LIMIT: u32 = 2000;

/// How old a half-finished walk may be and still be picked up where it
/// stopped.
///
/// The offsets in a partial pool are positions in a list Yahoo re-orders as
/// players are added and dropped. Resuming an old one at offset 300 skips
/// whatever has moved above that line and reads some of the rest twice, and
/// the walk then marks the result complete, so nothing ever notices. An hour
/// is short enough that the pool has not moved and long enough that a draft
/// night throttle is still resumable.
pub(crate) const POOL_RESUME_MAX_AGE_SECS: u64 = 3_600;

/// Whether a resumed pool holds the rows its own offsets say it read.
///
/// Yahoo publishes no total, so the check is against the walk's own
/// arithmetic: every page but the last was a full one, so a walk that has
/// reached offset N is holding at least N minus one page of rows. Fewer than
/// that and the cache this walk resumed from was not describing this pool —
/// which is exactly what a stale partial looks like once Yahoo has re-ordered
/// underneath it.
fn offsets_disagree(pool: &PlayerPool) -> Option<String> {
    let claimed = pool.next_start.saturating_sub(PAGE);
    let held = pool.players.len() as u32;
    (held < claimed).then(|| {
        format!(
            "the Yahoo player pool came back with {held} players where the {} pages read \
             should have held at least {claimed}; it is loading again from the start on the \
             next refresh",
            pool.next_start / PAGE
        )
    })
}

impl Engine {
    /// The league's player pool, resuming a walk an earlier load left partway
    /// through and caching whatever this one manages to add.
    pub async fn yahoo_pool(
        &self,
        client: &YahooClient,
        league_key: &str,
        force: bool,
    ) -> Result<(Vec<YahooPlayer>, Option<String>), String> {
        let name = cache_name(league_key, "players");
        let cached = self.read_cache_any_off_thread::<PlayerPool>(&name).await;
        if !force {
            if let Some((at, pool)) = &cached {
                let fresh = crate::engine::now_secs().saturating_sub(*at) < PLAYERS_TTL_SECS;
                if fresh && pool.complete {
                    return Ok((pool.players.clone(), None));
                }
            }
        }
        // A cache that is only half a pool is worth resuming even when it is
        // stale: those pages cost as much to fetch again as they did the
        // first time, and Yahoo is throttling precisely because of them.
        let resume = match &cached {
            Some((at, pool))
                if !pool.complete
                    && crate::engine::now_secs().saturating_sub(*at) < POOL_RESUME_MAX_AGE_SECS =>
            {
                pool.clone()
            }
            _ => PlayerPool::empty(),
        };
        let resumed_from = resume.next_start;
        let (mut pool, error) = client.pool_from(league_key, None, POOL_LIMIT, resume).await;
        let read_something = pool.next_start > resumed_from;
        // Only a resumed walk can be wrong about this: one that started at
        // page zero read every row it is holding.
        let disagreed = (resumed_from > 0)
            .then(|| offsets_disagree(&pool))
            .flatten();
        if disagreed.is_some() {
            // Not complete, whatever the last page said, and not resumable
            // either: appending to this cache again would carry the same hole
            // forward and double every row it does have. The next load starts
            // clean.
            pool.complete = false;
            self.write_cache_off_thread(&name, &PlayerPool::empty())
                .await;
        } else if read_something {
            self.write_cache_off_thread(&name, &pool).await;
        }
        match error {
            None => Ok((pool.players, disagreed)),
            // Some of the pool is a board with holes in it, and a board with
            // holes is worse than yesterday's whole one. Prefer the cache
            // when there is a complete one, and say so either way.
            Some(error) => match cached {
                Some((at, cache)) if cache.complete => {
                    let age = crate::engine::now_secs().saturating_sub(at) / 3600;
                    Ok((
                        cache.players,
                        Some(format!(
                            "Yahoo player pool refresh failed; using cache aged {age}h ({error})"
                        )),
                    ))
                }
                _ if !pool.players.is_empty() => Ok((
                    pool.players,
                    Some(format!(
                        "Yahoo sent only part of the player pool ({error}); \
                         the rest loads on the next refresh"
                    )),
                )),
                _ => Err(format!("Yahoo player pool: {error}")),
            },
        }
    }

    /// The league's player pool as the last load left it on disk.
    ///
    /// Never fetches, and never minds that the cache is stale or partial: the
    /// caller is a poll tick that wants a name and a position for a pick that
    /// has just been made, and yesterday's row for a player is a better answer
    /// than no row at all.
    pub async fn yahoo_cached_pool(&self, league_key: &str) -> Vec<YahooPlayer> {
        self.read_cache_any_off_thread::<PlayerPool>(&cache_name(league_key, "players"))
            .await
            .map(|(_, pool)| pool.players)
            .unwrap_or_default()
    }
}
