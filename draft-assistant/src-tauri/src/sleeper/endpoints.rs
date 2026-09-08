//! The documented endpoints, one method each. Split out of `sleeper.rs`,
//! which holds the client and its transport, so that file stays inside the
//! project's line limit.

use super::{
    Draft, League, LeagueUser, Pick, ProjectionRow, SleeperClient, SleeperUser, BASE, BASE_UNDOC,
    PLAYERS_TIMEOUT,
};
use crate::sleeper_error::SleeperError;

/// A `null` draft is Sleeper's way of saying the id is unknown.
fn draft_or_not_found(draft_id: &str, draft: Option<Draft>) -> Result<Draft, SleeperError> {
    draft.ok_or_else(|| {
        SleeperError::NotFound(format!(
            "draft {draft_id} not found (Sleeper returned null)"
        ))
    })
}

impl SleeperClient {
    /// Resolve a Sleeper username to its user id.
    ///
    /// Sleeper usernames are alphanumerics plus `_` and `-`; anything else is
    /// refused rather than escaped, because it would be interpolated into the
    /// request path.
    pub async fn user(&self, username: &str) -> Result<SleeperUser, SleeperError> {
        let username = username.trim();
        let legal = !username.is_empty()
            && username.len() <= 32
            && username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !legal {
            return Err(SleeperError::Invalid(format!(
                "'{username}' is not a valid Sleeper username"
            )));
        }
        let user: Option<SleeperUser> = self.get_json(&format!("{BASE}/user/{username}")).await?;
        user.ok_or_else(|| SleeperError::NotFound(format!("Sleeper user '{username}' not found")))
    }

    pub async fn league(&self, league_id: &str) -> Result<League, SleeperError> {
        let v: Option<League> = self.get_json(&format!("{BASE}/league/{league_id}")).await?;
        v.ok_or_else(|| {
            SleeperError::NotFound(format!(
                "league {league_id} not found (Sleeper returned null)"
            ))
        })
    }

    pub async fn draft(&self, draft_id: &str) -> Result<Draft, SleeperError> {
        let v: Option<Draft> = self.get_json(&format!("{BASE}/draft/{draft_id}")).await?;
        draft_or_not_found(draft_id, v)
    }

    /// The same resource with one attempt and a short deadline, for the poll
    /// tick, which reads it beside the picks and keeps the last one when it
    /// does not answer. See [`super::NOTE_TIMEOUT`].
    pub async fn draft_quick(&self, draft_id: &str) -> Result<Draft, SleeperError> {
        let v: Option<Draft> = self
            .get_json_quick(&format!("{BASE}/draft/{draft_id}"))
            .await?;
        draft_or_not_found(draft_id, v)
    }

    pub async fn picks(&self, draft_id: &str) -> Result<Vec<Pick>, SleeperError> {
        let v: Option<Vec<Pick>> = self
            .get_json(&format!("{BASE}/draft/{draft_id}/picks"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// All members of a league (for slot display names). One call.
    pub async fn league_users(&self, league_id: &str) -> Result<Vec<LeagueUser>, SleeperError> {
        let v: Option<Vec<LeagueUser>> = self
            .get_json(&format!("{BASE}/league/{league_id}/users"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// Optional member recovery during a draft tick: one bounded attempt.
    pub async fn league_users_quick(
        &self,
        league_id: &str,
    ) -> Result<Vec<LeagueUser>, SleeperError> {
        let users: Option<Vec<LeagueUser>> = self
            .get_json_quick(&format!("{BASE}/league/{league_id}/users"))
            .await?;
        Ok(users.unwrap_or_default())
    }

    /// Full player dictionary, unparsed: ~14.6 MB of JSON, cached on disk.
    ///
    /// Bytes rather than a `HashMap` because the caller parses it on the
    /// blocking pool — see `projections::players`.
    pub async fn players_bytes(&self) -> Result<Vec<u8>, SleeperError> {
        self.get_bytes_within(&format!("{BASE}/players/nfl"), Some(PLAYERS_TIMEOUT))
            .await
    }

    /// Undocumented: full-season raw-stat projections for one season.
    pub async fn season_projections(
        &self,
        season: u32,
    ) -> Result<Vec<ProjectionRow>, SleeperError> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=K&position[]=DEF&order_by=adp_ppr"
        );
        self.get_json(&url).await
    }

    /// Undocumented: one week's raw-stat projections (for per-game bonus modeling).
    pub async fn weekly_projections(
        &self,
        season: u32,
        week: u32,
    ) -> Result<Vec<ProjectionRow>, SleeperError> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}/{week}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=K&position[]=DEF"
        );
        self.get_json(&url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A server that answers every request with `response`, counting them.
    /// `delay` holds each answer back first.
    fn stub(response: &'static str, delay: Duration) -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a stub");
        let host = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer);
                std::thread::sleep(delay);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (host, hits)
    }

    /// `/draft` failing during a tick retried three times at eight seconds
    /// each: 25 seconds under a green sync badge, for a resource the tick
    /// keeps the last copy of anyway.
    #[tokio::test]
    async fn the_note_only_draft_call_is_asked_once_and_gives_up_inside_its_deadline() {
        let (host, hits) = stub(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Duration::ZERO,
        );
        SleeperClient::with_host(host)
            .draft_quick("1")
            .await
            .expect_err("a 503 is not a draft");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "one try, however retryable");

        // A server that takes longer than the note deadline is abandoned at
        // the deadline, not at the client's eight seconds.
        let (host, _) = stub(
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnull",
            super::super::NOTE_TIMEOUT + Duration::from_secs(4),
        );
        let started = Instant::now();
        SleeperClient::with_host(host)
            .draft_quick("1")
            .await
            .expect_err("a server that does not answer in time is not a draft");
        let took = started.elapsed();
        assert!(
            took < super::super::NOTE_TIMEOUT + Duration::from_secs(2),
            "gave up after {took:?}, past the note deadline"
        );
    }
}
