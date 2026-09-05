//! Every file under `tests/fixtures/yahoo/` still parses into something usable.
//!
//! `yahoo_parse.rs` asserts on particular values -- that league two is
//! `449.l.67890`, that the auction pick cost $47. Those tests are precise and
//! they are also easy to satisfy by accident: each one reads a couple of fields
//! out of one fixture, so a fixture edited down to a stub, or an object nested
//! one level too deep after a hand-merge, can leave most of it parsing to
//! empty defaults with every existing assertion still green.
//!
//! This walks the directory instead. Every file is read, sent through the
//! public parser entry point for its shape, and checked for the fields the
//! mapper downstream cannot work without. Two properties matter:
//!
//!   * a fixture that stops carrying real data fails here, loudly, naming the
//!     file; and
//!   * a *new* fixture file fails until it is listed below, so it cannot be
//!     added, used by one test, and then silently rot.
//!
//! Deliberately shape-only: no value in here is spelled out, because that is
//! `yahoo_parse.rs`'s job and duplicating it would make every fixture edit a
//! two-file change.

use draft_assistant_lib::yahoo_parse as parse;
use serde_json::Value;

/// What a fixture is a recording of, and so which parser reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    UserLeagues,
    League,
    Teams,
    /// Draft results with picks in them.
    DraftResults,
    /// Draft results from before the draft: correctly, legitimately empty.
    DraftResultsEmpty,
    /// A page of `/players`, or a team roster -- Yahoo returns the same shape.
    Players,
    /// `teams;out=roster`: every team's roster in one payload, which is where
    /// the keeper flag actually lives.
    TeamsWithRosters,
}

/// Where a fixture's *shape* came from.
///
/// This is not decoration. A hand-written fixture is not evidence that Yahoo
/// sends a field: the keeper flag on a draft result and the budget in an
/// auction's settings were both believed for exactly as long as the only
/// place they appeared was a file in this directory, and both turned out to
/// be absent from the live resource often enough to matter. Anything read out
/// of a `HandWritten` fixture needs a fallback for the leagues that do not
/// send it, and the fallback needs its own test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The shape is a real Yahoo response's, with the names, ids and numbers
    /// replaced by fictional ones.
    RecordedShape,
    /// Assembled by hand from a recorded fixture of the same resource, to
    /// stand in for a league nobody had a recording of.
    HandWritten,
}

/// Every fixture in the directory, and what it is. The walk below fails on any
/// file that is not in this list, which is the point: adding a fixture means
/// saying what it holds.
const FIXTURES: &[(&str, Shape, Source)] = &[
    (
        "user_leagues.json",
        Shape::UserLeagues,
        Source::RecordedShape,
    ),
    ("league_settings.json", Shape::League, Source::RecordedShape),
    // No auction league was recorded: this one is the plain settings payload
    // with `is_auction_draft` and `draft_budget` written into it. Yahoo does
    // not always send the budget, which is why `yahoo_map::derived_budget`
    // exists and is tested against results that carry no budget at all.
    (
        "league_settings_auction.json",
        Shape::League,
        Source::HandWritten,
    ),
    ("teams.json", Shape::Teams, Source::RecordedShape),
    (
        "teams_rosters.json",
        Shape::TeamsWithRosters,
        Source::RecordedShape,
    ),
    (
        "draft_results_predraft.json",
        Shape::DraftResultsEmpty,
        Source::RecordedShape,
    ),
    (
        "draft_results_partial.json",
        Shape::DraftResults,
        Source::RecordedShape,
    ),
    (
        "draft_results_complete.json",
        Shape::DraftResults,
        Source::RecordedShape,
    ),
    // Costs written onto a recorded result set by hand.
    (
        "draft_results_auction.json",
        Shape::DraftResults,
        Source::HandWritten,
    ),
    // `is_keeper` written onto a recorded result set by hand. The live
    // `draftresults` resource does not send it, which is why the keeper flags
    // are read off `teams_rosters.json`'s shape instead.
    (
        "draft_results_keepers.json",
        Shape::DraftResults,
        Source::HandWritten,
    ),
    ("players_page_0.json", Shape::Players, Source::RecordedShape),
    ("players_page_1.json", Shape::Players, Source::RecordedShape),
    ("team_roster.json", Shape::Players, Source::RecordedShape),
];

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("yahoo")
}

/// Every `.json` file actually on disk, sorted so a failure reads the same way
/// twice.
fn files_on_disk() -> Vec<String> {
    let dir = fixture_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").file_name())
        .filter_map(|name| name.to_str().map(str::to_string))
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
}

fn load(name: &str) -> Value {
    let path = fixture_dir().join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid JSON any more: {e}"))
}

/// The list and the directory describe the same set of files.
#[test]
fn every_fixture_on_disk_is_accounted_for_and_every_listed_one_exists() {
    let mut listed: Vec<&str> = FIXTURES.iter().map(|(name, _, _)| *name).collect();
    listed.sort_unstable();
    let on_disk = files_on_disk();
    let on_disk: Vec<&str> = on_disk.iter().map(String::as_str).collect();
    assert_eq!(
        listed, on_disk,
        "tests/fixtures/yahoo/ and the FIXTURES list disagree; a new fixture needs its shape \
         named here, and a deleted one needs removing"
    );
}

#[test]
fn a_leagues_listing_parses_into_leagues_that_can_be_asked_for() {
    let leagues = parse::user_leagues(&load("user_leagues.json"));
    assert!(!leagues.is_empty(), "user_leagues.json parsed to nothing");
    for league in &leagues {
        // The key is the only thing every later call is built from; without it
        // the league cannot be fetched at all.
        assert!(
            league.league_key.contains(".l."),
            "league_key is not a Yahoo league key: {:?}",
            league.league_key
        );
        assert!(!league.league_id.is_empty(), "no league_id");
        assert!(!league.name.is_empty(), "no name");
        assert!(!league.season.is_empty(), "no season");
        assert!(league.num_teams > 0, "a league with no teams");
        assert!(!league.draft_status.is_empty(), "no draft_status");
    }
}

#[test]
fn the_league_resource_parses_with_the_settings_the_mapper_needs() {
    for (name, ..) in FIXTURES
        .iter()
        .filter(|(_, shape, _)| *shape == Shape::League)
    {
        one_league_parses(name);
    }
}

fn one_league_parses(name: &str) {
    let league = parse::league(&load(name)).unwrap_or_else(|| panic!("{name} parsed to no league"));
    assert!(league.league_key.contains(".l."));
    assert!(!league.league_id.is_empty());
    assert!(!league.name.is_empty());
    assert!(!league.season.is_empty());
    assert!(league.num_teams > 0);
    assert!(!league.draft_status.is_empty());
    // These two are the whole reason the settings resource is fetched at all:
    // without them there is no roster to fill and no way to score anyone.
    assert!(
        !league.roster_positions.is_empty(),
        "no roster_positions -- the settings are not folded in"
    );
    assert!(
        league
            .roster_positions
            .iter()
            .all(|slot| { !slot.position.is_empty() && slot.count > 0 }),
        "a roster slot with no position or a count of zero"
    );
    assert!(
        !league.stat_modifiers.is_empty(),
        "no stat_modifiers -- nothing could be scored"
    );
    // An auction board is unreadable without the budget the bids are
    // measured against, so a fixture written to carry one has to carry it.
    // Only a hand-written one is held to that: a recorded auction league may
    // legitimately send no budget at all, and that is the case
    // `yahoo_map::derived_budget` covers.
    let hand_written = FIXTURES
        .iter()
        .any(|(file, _, source)| *file == name && *source == Source::HandWritten);
    if league.is_auction_draft && hand_written {
        assert!(
            league.draft_budget.unwrap_or(0) > 0,
            "{name}: an auction league with no draft_budget"
        );
    }
}

/// The rosters payload is where a keeper is flagged, so it has to parse into
/// players that carry the flag -- and into *every* team's, not just the first.
#[test]
fn the_rosters_payload_names_every_teams_players_and_says_who_is_kept() {
    for (name, ..) in FIXTURES
        .iter()
        .filter(|(_, shape, _)| *shape == Shape::TeamsWithRosters)
    {
        let payload = load(name);
        let teams = parse::teams(&payload);
        assert!(teams.len() > 1, "{name}: fewer than two teams to walk");
        let rosters = parse::rosters(&payload);
        assert!(
            rosters.len() >= teams.len(),
            "{name}: {} players across {} teams -- the walk stopped at the first roster",
            rosters.len(),
            teams.len()
        );
        for player in &rosters {
            assert!(
                player.player_key.contains(".p."),
                "{name}: a roster row with no player key: {:?}",
                player.player_key
            );
        }
        // The whole reason this resource is fetched: at least one row has to
        // carry a keeper decision, or the fixture proves nothing.
        assert!(
            rosters.iter().any(|player| player.is_keeper == Some(true)),
            "{name}: no player is flagged as kept"
        );
        assert!(
            rosters.iter().any(|player| player.is_keeper.is_none()),
            "{name}: every row carries a flag, so the silent case is untested"
        );
    }
}

#[test]
fn the_teams_listing_parses_into_addressable_teams() {
    let teams = parse::teams(&load("teams.json"));
    assert!(!teams.is_empty(), "teams.json parsed to nothing");
    for team in &teams {
        assert!(
            team.team_key.contains(".t."),
            "team_key is not a Yahoo team key: {:?}",
            team.team_key
        );
        assert!(!team.team_id.is_empty(), "no team_id");
        assert!(!team.name.is_empty(), "no name");
    }
}

#[test]
fn a_predraft_result_set_is_empty_rather_than_full_of_blanks() {
    // Yahoo answers `/draftresults` before the draft with the resource present
    // and no picks in it. Parsing that into a list of default-constructed picks
    // would have the poller believe the draft had started.
    assert!(
        parse::draft_results(&load("draft_results_predraft.json")).is_empty(),
        "the predraft fixture produced picks"
    );
}

#[test]
fn every_recorded_draft_result_parses_into_ordered_picks() {
    for (name, ..) in FIXTURES
        .iter()
        .filter(|(_, shape, _)| *shape == Shape::DraftResults)
    {
        let picks = parse::draft_results(&load(name));
        assert!(!picks.is_empty(), "{name} parsed to no picks");
        for pick in &picks {
            assert!(pick.pick > 0, "{name}: a pick numbered {}", pick.pick);
            assert!(pick.round > 0, "{name}: a pick in round {}", pick.round);
            assert!(
                pick.team_key.contains(".t."),
                "{name}: pick {} has no team: {:?}",
                pick.pick,
                pick.team_key
            );
        }
        // Overall numbering is what the board is rebuilt from, so a duplicate
        // or a gap is a corrupt fixture rather than an unusual draft.
        let mut numbers: Vec<u32> = picks.iter().map(|p| p.pick).collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(
            numbers.len(),
            picks.len(),
            "{name}: two picks share an overall number"
        );
        // At least one pick must name a player, or the fixture records a draft
        // in which nobody was taken.
        assert!(
            picks.iter().any(|p| p.player_key.contains(".p.")),
            "{name}: no pick names a player"
        );
    }
}

#[test]
fn every_player_page_parses_into_players_the_crosswalk_can_match_on() {
    for (name, ..) in FIXTURES
        .iter()
        .filter(|(_, shape, _)| *shape == Shape::Players)
    {
        let page = parse::players(&load(name));
        assert!(!page.players.is_empty(), "{name} parsed to no players");
        assert_eq!(
            page.count,
            page.players.len(),
            "{name}: the reported count and the parsed rows disagree, so paging would stop early \
             or loop"
        );
        for player in &page.players {
            assert!(
                player.player_key.contains(".p."),
                "{name}: player_key is not a Yahoo player key: {:?}",
                player.player_key
            );
            assert!(!player.player_id.is_empty(), "{name}: no player_id");
            // Name, team and position are the three the Sleeper crosswalk
            // matches on; any one of them missing makes the row unmatchable.
            assert!(
                !player.full_name.is_empty(),
                "{name}: {} has no name",
                player.player_key
            );
            assert!(
                !player.display_position.is_empty(),
                "{name}: {} has no position",
                player.full_name
            );
            assert!(
                !player.eligible_positions.is_empty(),
                "{name}: {} is eligible nowhere",
                player.full_name
            );
        }
    }
}
