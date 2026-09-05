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
//!    throttle stopped on instead of at page zero.
//! 3. A walk that fails having read nothing new falls back to whatever was on
//!    disk, with a warning, exactly as the other Yahoo reads do.
//! 4. Only a failure with nothing behind it at all takes the load down.

use crate::engine::{Engine, PLAYERS_TTL_SECS};
use crate::engine_yahoo::cache_name;
use crate::yahoo::YahooClient;
use crate::yahoo_pool::PlayerPool;
use crate::yahoo_types::YahooPlayer;

/// Yahoo's pages are 25 players each, so the pool is the most expensive thing
/// in a load. This is the stop of last resort; a real league's pool ends
/// first.
pub(crate) const POOL_LIMIT: u32 = 2000;

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
            Some((_, pool)) if !pool.complete => pool.clone(),
            _ => PlayerPool::empty(),
        };
        let resumed_from = resume.next_start;
        let (pool, error) = client.pool_from(league_key, None, POOL_LIMIT, resume).await;
        let read_something = pool.next_start > resumed_from;
        if read_something {
            self.write_cache_off_thread(&name, &pool).await;
        }
        match error {
            None => Ok((pool.players, None)),
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
}
