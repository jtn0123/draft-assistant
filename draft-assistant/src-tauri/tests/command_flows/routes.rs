//! The stub Sleeper that `command_flows.rs` runs against: one league, one
//! draft, one account, and every endpoint the loaders touch for them.
//!
//! Alongside that sit a handful of scenario leagues, each one wired to
//! misbehave in exactly one way — an answer held back until a test has
//! switched leagues under it, a pick list that vanishes mid-draft, a draft
//! that comes back with no teams. Every scenario league is driven by exactly
//! one test, so the flags below cannot be tripped by a test running beside it.

use crate::stub;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A Sleeper league id is a long run of digits, and `add_league` insists on
/// one, so the fixture uses a realistic id rather than "league-1".
pub const LEAGUE_ID: &str = "1000000000000000001";
pub const DRAFT_ID: &str = "2000000000000000002";
pub const USER_ID: &str = "3000000000000000003";

/// The league whose picks are held back while a test switches away from it.
pub const LEAGUE_SWITCH: &str = "1000000000000000012";
pub const DRAFT_SWITCH: &str = "2000000000000000012";
/// The league whose pick list goes missing mid-draft.
pub const LEAGUE_VANISH: &str = "1000000000000000013";
pub const DRAFT_VANISH: &str = "2000000000000000013";
/// The league whose draft comes back saying it has no teams.
pub const LEAGUE_BROKEN: &str = "1000000000000000014";
pub const DRAFT_BROKEN: &str = "2000000000000000014";
/// The league a full rebuild is run for while a test switches away from it.
pub const LEAGUE_REBUILD: &str = "1000000000000000015";
pub const DRAFT_REBUILD: &str = "2000000000000000015";
/// The league whose live refresh is held back while a test switches away.
pub const LEAGUE_LIVE: &str = "1000000000000000016";
pub const DRAFT_LIVE: &str = "2000000000000000016";
/// The league the background poller is watching when a test switches away.
pub const LEAGUE_TICK: &str = "1000000000000000017";
pub const DRAFT_TICK: &str = "2000000000000000017";
/// The league that agrees a pick trade in the middle of its own draft.
pub const LEAGUE_TRADE: &str = "1000000000000000018";
pub const DRAFT_TRADE: &str = "2000000000000000018";
/// The league whose pick list comes back with a hole in it mid-draft.
pub const LEAGUE_HOLE: &str = "1000000000000000019";
pub const DRAFT_HOLE: &str = "2000000000000000019";

/// Every league this stub serves, with the draft it points at.
const LEAGUES: [(&str, &str); 9] = [
    (LEAGUE_ID, DRAFT_ID),
    (LEAGUE_SWITCH, DRAFT_SWITCH),
    (LEAGUE_VANISH, DRAFT_VANISH),
    (LEAGUE_BROKEN, DRAFT_BROKEN),
    (LEAGUE_REBUILD, DRAFT_REBUILD),
    (LEAGUE_LIVE, DRAFT_LIVE),
    (LEAGUE_TICK, DRAFT_TICK),
    (LEAGUE_TRADE, DRAFT_TRADE),
    (LEAGUE_HOLE, DRAFT_HOLE),
];

/// One endpoint a test can stop mid-answer.
///
/// Holding a request open is the only way to reach the code that decides what
/// to do with an answer that arrives *after* the user has moved on: the test
/// makes the switch while the request is genuinely in flight, and then lets it
/// finish.
pub struct Gate {
    /// How many requests are sitting in this gate right now, waiting to be
    /// let through. Only counted while the gate is held, which makes a
    /// non-zero reading the one thing a switch test needs to know before it
    /// switches: a request is genuinely in flight *inside* the gate.
    waiting: AtomicUsize,
    held: AtomicBool,
    /// A request really did sit in this gate while it was held, and was let
    /// out by `release` rather than by the timeout. This is the whole point
    /// of the gate, and without it a switch test could pass having never
    /// entered the code it is about: the request comes and goes before the
    /// test gets to `hold`, the switch happens with nothing in flight, and
    /// the discard path is never reached.
    raced: AtomicBool,
    /// A request gave up waiting. The gate never blocks forever, so a test
    /// that fails before releasing fails rather than hanging; but a timeout
    /// means the race did not happen the way the test describes it, and it
    /// must be told rather than quietly carrying on.
    timed_out: AtomicBool,
}

impl Gate {
    const fn new() -> Self {
        Self {
            waiting: AtomicUsize::new(0),
            held: AtomicBool::new(false),
            raced: AtomicBool::new(false),
            timed_out: AtomicBool::new(false),
        }
    }

    /// Whether a held request was let through by the test rather than by the
    /// timeout: the proof that the race under test actually happened.
    pub fn raced(&self) -> bool {
        self.raced.load(Ordering::SeqCst)
    }

    /// Whether a request in this gate ran out of patience.
    pub fn timed_out(&self) -> bool {
        self.timed_out.load(Ordering::SeqCst)
    }

    /// Stop answering until `release`, and start this round's evidence over.
    ///
    /// Resetting here rather than leaving the flags from a previous hold is
    /// what lets `raced` mean "during *this* hold".
    pub fn hold(&self) {
        self.waiting.store(0, Ordering::SeqCst);
        self.raced.store(false, Ordering::SeqCst);
        self.timed_out.store(false, Ordering::SeqCst);
        self.held.store(true, Ordering::SeqCst);
    }

    pub fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
    }

    /// How many requests are held in this gate right now. A test waits for
    /// this to be non-zero rather than counting requests served: a request
    /// that came and went before `hold` was called is not the one in flight,
    /// and switching on the strength of it left the code under test never
    /// entered.
    pub fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }

    /// Called from the stub's own thread: count the request, then wait for the
    /// test to let it through. Never waits forever — a test that fails before
    /// releasing should fail, not hang.
    fn wait(&self) {
        // Not held: this request is not the one the test is holding, and it
        // is not evidence of anything either way.
        if !self.held.load(Ordering::SeqCst) {
            return;
        }
        self.waiting.fetch_add(1, Ordering::SeqCst);
        for _ in 0..2000 {
            if !self.held.load(Ordering::SeqCst) {
                self.raced.store(true, Ordering::SeqCst);
                self.waiting.fetch_sub(1, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.timed_out.store(true, Ordering::SeqCst);
        self.waiting.fetch_sub(1, Ordering::SeqCst);
    }
}

pub static SWITCH_PICKS: Gate = Gate::new();
pub static REBUILD_PICKS: Gate = Gate::new();
pub static LIVE_MATCHUPS: Gate = Gate::new();
pub static TICK_PICKS: Gate = Gate::new();

/// Set once `LEAGUE_VANISH` has been loaded: from then on its pick list comes
/// back as `null`, the way Sleeper's really does now and then.
pub static PICKS_VANISHED: AtomicBool = AtomicBool::new(false);
/// Set once `LEAGUE_BROKEN` has been loaded: from then on its draft reports
/// zero teams and zero rounds.
pub static DRAFT_IS_BROKEN: AtomicBool = AtomicBool::new(false);
/// Set once `LEAGUE_TRADE` has been loaded: from then on its traded-pick list
/// carries the trade the managers agreed while the draft was running.
pub static TRADE_AGREED: AtomicBool = AtomicBool::new(false);
/// What `LEAGUE_HOLE` answers `/picks` with: the full list (0), the list with
/// pick 2 missing but 3 and 4 still in it (1), or the list with the last
/// pick taken back (2).
pub static HOLE_PICKS: AtomicUsize = AtomicUsize::new(0);
pub const HOLE_FULL: usize = 0;
pub const HOLE_MISSING_ONE: usize = 1;
pub const HOLE_LAST_UNDONE: usize = 2;

/// Four picks of a two-team, three-round draft, in a form the hole scenario
/// can drop rows from.
fn hole_picks(without: &[u32]) -> String {
    let rows: Vec<String> = [(1, 1, "rb-1"), (2, 2, "wr-1"), (3, 2, "qb-1"), (4, 1, "x-4")]
        .iter()
        .filter(|(pick_no, _, _)| !without.contains(pick_no))
        .map(|(pick_no, slot, player)| {
            format!(
                r#"{{"round": {}, "pick_no": {pick_no}, "draft_slot": {slot}, "player_id": "{player}"}}"#,
                (pick_no - 1) / 2 + 1
            )
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// Slot 1's third-round pick, sold to slot 2's roster mid-draft. Rosters are
/// deliberately not equal to slots (slot 1 is roster 10, slot 2 is roster 20)
/// so a stub that confused the two would fail.
const MID_DRAFT_TRADE: &str = r#"[{"season": "2026", "round": 3, "roster_id": 10,
                                   "owner_id": 20, "previous_owner_id": 10}]"#;

fn league_json(league_id: &str, draft_id: &str) -> String {
    format!(
        r#"{{"league_id": "{league_id}", "name": "Command League", "season": "2026",
             "status": "drafting", "total_rosters": 2,
             "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
             "scoring_settings": {{"rec": 1.0, "rush_yd": 0.1, "rush_td": 6.0,
                                   "rec_yd": 0.1, "rec_td": 6.0, "pass_yd": 0.04,
                                   "pass_td": 4.0}},
             "draft_id": "{draft_id}", "settings": {{"playoff_week_start": 15}}}}"#
    )
}

fn draft_json(draft_id: &str, teams: u32, rounds: u32) -> String {
    // The slot-to-roster map is what a traded pick is translated through, so
    // every draft here carries one.
    format!(
        r#"{{"draft_id": "{draft_id}", "status": "drafting", "type": "snake",
             "settings": {{"teams": {teams}, "rounds": {rounds}}},
             "draft_order": {{"{USER_ID}": 1}}, "season": "2026",
             "slot_to_roster_id": {{"1": 10, "2": 20}}}}"#
    )
}

/// One pick sitting past the clock, which is what makes it a keeper — and so
/// something the poller writes to disk under whichever draft is loaded.
const KEEPER_PICK: &str = r#"[{"round": 2, "pick_no": 3, "draft_slot": 1,
                               "player_id": "wr-1"}]"#;

const ONE_PICK: &str = r#"[{"round": 1, "pick_no": 1, "draft_slot": 1,
                            "player_id": "rb-1"}]"#;

const PLAYERS: &str = r#"{
    "qb-1": {"full_name": "Command Passer", "position": "QB", "team": "AAA"},
    "rb-1": {"full_name": "Command Runner", "position": "RB", "team": "BBB"},
    "wr-1": {"full_name": "Command Catcher", "position": "WR", "team": "CCC"}
}"#;

const SEASON_ROWS: &str = r#"[
    {"player_id": "qb-1", "stats": {"pass_yd": 4200.0, "pass_td": 32.0, "adp_ppr": 14.0},
     "player": {"position": "QB", "team": "AAA"}},
    {"player_id": "rb-1", "stats": {"rush_yd": 1200.0, "rush_td": 10.0, "rec": 40.0, "adp_ppr": 3.0},
     "player": {"position": "RB", "team": "BBB"}},
    {"player_id": "wr-1", "stats": {"rec_yd": 1300.0, "rec_td": 9.0, "rec": 95.0, "adp_ppr": 5.0},
     "player": {"position": "WR", "team": "CCC"}}
]"#;

const ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "3000000000000000003", "players": ["qb-1", "rb-1"],
     "starters": ["qb-1"], "settings": {"wins": 1, "losses": 0, "fpts": 120}},
    {"roster_id": 2, "owner_id": "other", "players": ["wr-1"],
     "starters": ["wr-1"], "settings": {"wins": 0, "losses": 1, "fpts": 90}}
]"#;

/// Everything under `/v1/league/{id}`, which every league here answers the
/// same way apart from the league document itself.
fn league_reply(league_id: &str, draft_id: &str, rest: &str) -> Option<stub::Reply> {
    let ok = |body: String| Some((200u16, body));
    match rest {
        "" => ok(league_json(league_id, draft_id)),
        "/users" => ok(format!(
            r#"[{{"user_id": "{USER_ID}", "display_name": "Ada"}}]"#
        )),
        "/rosters" => ok(ROSTERS.to_string()),
        "/winners_bracket" => ok("[]".to_string()),
        _ if rest.starts_with("/matchups") => {
            if league_id == LEAGUE_LIVE {
                LIVE_MATCHUPS.wait();
            }
            ok("[]".to_string())
        }
        _ if rest.starts_with("/transactions") => ok("[]".to_string()),
        _ => None,
    }
}

/// Everything under `/v1/draft/{id}`, where the scenario drafts differ.
fn draft_reply(draft_id: &str, rest: &str) -> Option<stub::Reply> {
    let ok = |body: String| Some((200u16, body));
    match rest {
        "" => {
            let broken = draft_id == DRAFT_BROKEN && DRAFT_IS_BROKEN.load(Ordering::SeqCst);
            match broken {
                true => ok(draft_json(draft_id, 0, 0)),
                false => ok(draft_json(draft_id, 2, 3)),
            }
        }
        "/picks" => match draft_id {
            DRAFT_SWITCH => {
                SWITCH_PICKS.wait();
                ok(KEEPER_PICK.to_string())
            }
            DRAFT_TICK => {
                TICK_PICKS.wait();
                ok(KEEPER_PICK.to_string())
            }
            DRAFT_REBUILD => {
                REBUILD_PICKS.wait();
                ok("[]".to_string())
            }
            // A `null` body is what Sleeper actually serves when it loses the
            // list, and it parses as "no picks at all".
            DRAFT_VANISH if PICKS_VANISHED.load(Ordering::SeqCst) => ok("null".to_string()),
            DRAFT_VANISH => ok(ONE_PICK.to_string()),
            DRAFT_HOLE => match HOLE_PICKS.load(Ordering::SeqCst) {
                HOLE_MISSING_ONE => ok(hole_picks(&[2])),
                HOLE_LAST_UNDONE => ok(hole_picks(&[4])),
                _ => ok(hole_picks(&[])),
            },
            _ => ok("[]".to_string()),
        },
        "/traded_picks" => match draft_id {
            DRAFT_TRADE if TRADE_AGREED.load(Ordering::SeqCst) => ok(MID_DRAFT_TRADE.to_string()),
            _ => ok("[]".to_string()),
        },
        _ => None,
    }
}

pub fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: String| Some((200u16, body));
    if path == "/v1/players/nfl" {
        return ok(PLAYERS.to_string());
    }
    if path == "/v1/state/nfl" {
        return ok(r#"{"season": "2026", "week": 1, "display_week": 1}"#.to_string());
    }
    if path.starts_with("/scores/nfl/") {
        return ok("[]".to_string());
    }
    if let Some(rest) = path.strip_prefix("/projections/nfl/2026") {
        return match rest.is_empty() {
            true => ok(SEASON_ROWS.to_string()),
            false => ok("[]".to_string()),
        };
    }
    for (league_id, draft_id) in LEAGUES {
        if let Some(rest) = path.strip_prefix(&format!("/v1/league/{league_id}")) {
            return league_reply(league_id, draft_id, rest);
        }
        if let Some(rest) = path.strip_prefix(&format!("/v1/draft/{draft_id}")) {
            return draft_reply(draft_id, rest);
        }
    }
    match path {
        "/v1/user/ada" => ok(format!(
            r#"{{"user_id": "{USER_ID}", "username": "ada", "display_name": "Ada"}}"#
        )),
        _ if path.starts_with(&format!("/v1/user/{USER_ID}/leagues/nfl/")) => {
            ok(format!("[{}]", league_json(LEAGUE_ID, DRAFT_ID)))
        }
        _ => None,
    }
}
