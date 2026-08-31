# Draft Assistant

Local-first fantasy football assistant for Sleeper leagues, with two screens:
a **draft board** for draft night and a **season screen** for the rest of the
year. You play in Sleeper as normal; this app is a read-only second screen that
polls the public Sleeper API and answers under **your league's exact scoring
rules**.

Built with Tauri 2: Rust core engine + React/TypeScript-strict frontend.
Desktop (macOS) now; the same core compiles into an Android build later — no
server anywhere.

## What it does

### Draft screen

- **Custom scoring from data, not code.** The league's `scoring_settings` map
  is dot-multiplied with Sleeper's raw-stat projections (same key space), so
  6-pt passing TDs, full PPR, and DEF points-allowed buckets all price in
  automatically. Per-game yardage bonuses are modeled as expected points from
  weekly projections (normal approximation per game).
- **VORP against real league demand.** Flex demand is allocated to the
  positions that actually hold the best remaining players; replacement level
  falls out of roster shape × team count (this league: 98 RB/WR/TE startable).
- **Live draft tracking.** Polls `GET /draft/{id}/picks` every 3s; on-the-clock
  banner, all 14 rosters, tier alerts, position-run detection, recent picks.
- **Survival odds.** P(player lasts to your next pick) from ADP with a widening
  sigma — built for snake gaps like 27→30 and 55→58.
- **Recommendations with reasons.** Deterministic `balanced` and `safe` modes;
  every suggestion lists its auditable reasons (VORP, roster need, tier
  scarcity, survival, ADP value).
- **Manual pick fallback + undo** if the API lags or the draft is offline.
  API picks always win over manual ones.
- **AI-readable by design.** One call (`get_state` command, "Export state"
  button, or the `dump_state` CLI) emits the entire draft state as JSON — the
  same struct the UI renders. Point an LLM at it; nothing needs to scrape the UI.
- **Multi-league.** Leagues are stored in config; switching is a config value,
  never a code change.

### Season screen

- **This week's matchup**, slot by slot against your opponent, with the gap
  signed from your side. Toggle **Best / Set** to compare the lineup you have
  set against the best one available, so points left on the bench are visible
  before kickoff, not after.
- **Live scoreboard** on a 30s poll during games, with each game's TV network.
- **Standings and playoff odds** from a simulation over the remaining
  schedule, using best-lineup projections per team.
- **Waiver targets** ranked by what they would actually add to your starting
  lineup — a high scorer who still would not crack your eleven scores zero —
  with a suggested FAAB bid and how many rivals the same player would help.
- **Trade ideas** that improve both rosters, plus every completed trade in the
  league (including ones still in review) and a league activity feed.
- **Trends**: each team's projected strength over time.

### Ask Claude

A chat panel on either screen that sees the current board or matchup. See
[Ask Claude](#ask-claude) below for the two ways it can connect.

## Run (dev)

```bash
npm install
npm run tauri dev
```

## Build (release .app)

```bash
npm run tauri build
# → src-tauri/target/release/bundle/macos/Draft Assistant.app
```

## Headless state dump / simulation

```bash
cd src-tauri
cargo run --bin dump_state -- <league_id> [sleeper_username] [out.json] [--simulate N]
```

`--simulate N` fakes the first N picks (market drafts by ADP, your slots take
the balanced recommendation) to exercise mid-draft state without a live draft.

## Browser preview

The UI degrades to a read-only preview when opened in a plain browser (vite dev
server on :1420). It renders two captured dumps, and **both must be regenerated
together** or the preview shows a current draft board beside a stale season
screen:

```
cargo run --bin dump_state  -- <league_id> [username] ../public/dev-fixture.json
cargo run --bin dump_season -- <league_id> [username] ../public/dev-season-fixture.json
```

Both dumps read through an on-disk cache of their own — not the app's, but a
separate one under the system temp directory. To force genuinely fresh data,
delete it first:

```bash
rm -rf "${TMPDIR:-/tmp}/draft-assistant-cli"
```

On macOS `$TMPDIR` is a per-user folder under `/var/folders/...`, not `/tmp`,
which is why the plain `/tmp` path does not work here. That directory also
holds the Trends history snapshots (`history_<league_id>.json`), so clearing it
resets the CLI's trend lines too — the app's own history, under Application
Support, is untouched.

## Data sources

- `api.sleeper.app/v1` — league, draft, picks, players (documented, no auth)
- `api.sleeper.app/projections/nfl/{season}[/{week}]` — raw-stat projections +
  ADP (undocumented; responses are cached and parsed defensively)

Caches live in the app data dir (`~/Library/Application Support/
com.justin.draft-assistant/`): players 24h TTL, projections 6h TTL.
"Refresh data" forces a refetch and board rebuild.

## How it stays live

Nothing is pushed to this app — it asks Sleeper on a timer. There are **two
independent timers**, one per screen, and each only tells the UI when something
actually changed. That last part is the usual answer to "why didn't the screen
update?": the tick ran, nothing had moved, so no event was sent.

### The two pollers

| | Draft poller | Season poller |
|---|---|---|
| Started by | `start_polling`, from `App.tsx` when a draft is open | `start_season_polling`, from `session.ts` while the Season tab is showing |
| Loop lives in | `commands_draft.rs` | `commands_season.rs` |
| Decisions live in | `poll::DraftPollMemory` | `poll::season_tick` |
| Interval | 3s (caller may ask for 2–60) | 30s (caller may ask for 10–300) |
| Each tick fetches | picks + draft status, in parallel | this week's matchups + the NFL scoreboard + all rosters, in parallel |
| Always emits | `poll-health` | `season-poll-health` |
| Emits data only when | the picks or the draft status changed | either side's live total moved |
| Data event | `draft-updated` (a whole `DraftView`) | `season-updated` (a whole `SeasonView`) |

The health event goes out on **every** tick, pass or fail. That is deliberate:
a failed tick has no new data to send, so without a separate health event a
screen that has quietly stopped receiving updates looks identical to one where
nothing is happening. `PollHealthMemory` keeps three facts — when a request
last worked, how many have failed in a row since, and the most recent reason —
and the sync badge reads them.

### What counts as "changed"

- **Draft.** Not just "are there more picks". The poller keeps a count *and* a
  hash of which player sits at each pick number, so a commissioner editing a
  pick — same count, different player — still reaches the screen. The draft's
  status moving (`pre_draft` → `drafting` → `complete`) also counts, because it
  changes what the screen shows even with no new pick. The first tick always
  counts, so the initial state gets through.
- **Season.** `LiveEmitGate` compares both sides' live point totals, rounded to
  hundredths. Identical totals means no event. The season view is large and the
  whole panel re-renders on arrival, so sending an unchanged one is pure cost.

### The three tiers of cache

| Tier | Holds | Lives for | Cleared by |
|---|---|---|---|
| On disk | the players dictionary | 24h | "Refresh data", or the TTL |
| On disk | season projections | 6h | "Refresh data", or the TTL |
| On disk | the per-league week sweep (every week's pairings and points-to-date) | 6h | a `load_season(force)`, or the TTL |
| On disk | last season's final standings | 30 days | a `load_season(force)`, or the TTL |
| In memory | `AnalysisCache` — the expensive half of the season view | 20 season ticks (~10 min at the default 30s) | its own tick count, or the poller being restarted |
| In memory | `stableAvailable` in `boardIdentity.ts` — the last available-players array | until the board's contents actually differ | a genuinely different board |

`AnalysisCache` is the one worth knowing about. Rebuilding a season view from
scratch means roughly 1,600 lineup solves, a playoff simulation and a trade
search — and none of that can change because somebody scored a touchdown. So
the first tick computes it and the next nineteen reuse it. What it carries:

- standings and playoff odds
- waiver targets
- trade ideas
- the league activity feed's transaction half
- completed trades
- the Trends series

What it deliberately does **not** carry, and so is recomputed every single tick:
this week's matchup and start/sit calls, the live scoreboard, your roster, and
the empty-starter-slot warnings at the head of the activity feed — those come
off rosters the tick just refreshed, so a stale copy would be wrong.

The view reports `analysis_as_of_secs`, the moment the reused half was actually
computed, so the screen can admit that the waiver list is a few minutes old
while the score beside it is current.

### Retiring an old loop

Both loops are detached background tasks, so stopping one is a matter of
convincing it to exit on its own. Each poller has a counter on `AppState`
(`poll_generation` / `season_generation`). Starting a poller increments the
counter and hands the new loop that number; at the top of every tick a loop
compares its number to the current one and exits if they differ. So asking to
start a second time replaces the running loop rather than doubling it, and a
loop whose league was closed underneath it retires itself within one interval.
`stop_polling` / `stop_season_polling` flip a separate flag that has the same
effect.

Two things follow from this that are easy to trip over:

- Starting a poller **never fails**. Asking before a league is open just leaves
  a loop that picks one up as soon as there is one. So if the UI ever reports
  that live updates are not running, it really does mean that.
- The season poller keeps running on the backend's own schedule for as long as
  the tab is showing. `session.ts` starts it once on open and stops it on
  close, and does not restart it per update — restarting would reset the
  30-second timer on every tick.

## Layout

```
src/                        React + TS strict UI
  App.tsx                   screen switching, polling, shared state
  api.ts                    the IPC surface (+ browser-fixture fallback)
  components/               Board, ThisWeek, SeasonTabs, Trends, Games, Chat
  avatars.ts, zoom.ts       useSyncExternalStore modules
src-tauri/src/
  shared
    sleeper.rs              API client + response types
    sleeper_error.rs        what a request can fail with, and whether retrying helps
    engine.rs               caching, config, league loading
    cache.rs                the cache-file envelope: read, TTL check, atomic write
    projections.rs          projection fetch + stale-cache fallback
    roster.rs               slot rules (flex eligibility, draftable positions)
    scoring.rs              data-driven scorer + per-game bonus model
    headshots.rs            on-disk image cache (players + manager avatars)
    secrets.rs              API key in the macOS Keychain
    state.rs                Tauri-managed app state
    poll.rs                 what each poll tick decides — see "How it stays live"
    mock_league.rs          league settings invented for a mock draft, which has none
  draft screen
    draft.rs                snake math, rosters, survival probabilities
    board.rs                scored board assembly (incl. bye inference)
    valuation.rs            replacement levels, VORP, tiers
    recommend.rs            deterministic recommendation modes
    view.rs                 DraftView: the one state struct (UI + AI dump)
    simulation.rs           deterministic draft simulation
  season screen
    season.rs               SeasonView + SeasonAnalysis; orchestrates the sections
    season_types.rs         the view's plain data structs, kept out of season.rs
    season_view_matchup.rs  head-to-head rows + start/sit calls
    season_view_live.rs     live scoreboard for the two set lineups
    season_view_standings.rs standings + playoff odds
    season_view_market.rs   waiver targets + trade ideas
    season_view_feeds.rs    activity, completed trades, trends
    season_calls.rs         the words around a start/sit call: why, and when it locks
    season_injury.rs        Sleeper's dozen injury spellings cut down to three tags
    season_lookup.rs        player name/position/team/injury lookup
    season_api.rs           season-only Sleeper endpoints + DTOs
    season_engine.rs        season load, week sweep, live refresh
    season_sources.rs       per-feed freshness, so the badge can name what broke
    season_lineup.rs        optimal lineup solver
    season_odds.rs          playoff-odds simulation
    season_moves.rs         waiver targets by marginal lineup gain
    season_trades.rs        trade ideas
    season_deals.rs         completed trades, both sides named
    season_activity.rs      league activity feed
    season_live.rs          live scoreboard
    season_history.rs       team-strength snapshots, kept per league on disk
    season_trends_view.rs   the Trends tab: the series + why each line moved
    weekly.rs               weekly projection lookup
  ask claude
    chat.rs                 Anthropic Messages API route
    chat_cli.rs             Claude Code CLI route
    chat_context.rs         what Claude is shown per screen
  commands_draft.rs         draft commands + the 3s pick poller
  commands_season.rs        season commands + the 30s live poller
  commands_chat.rs          chat commands
  lib.rs                    the module list + command registration
  main.rs                   the desktop binary; hands straight off to lib.rs
  bin/dump_state.rs         headless draft dump + simulator
  bin/dump_season.rs        headless season dump
```

All files ≤ 500 LOC by project convention, enforced by `scripts/check-loc.mjs`
in `npm run verify`.

## Ask Claude

The chat panel reaches Claude one of two ways, picked in the panel itself:

- **Claude Code** — runs the `claude` CLI already installed on this Mac, signed
  in with your Claude subscription. No API key needed. Only offered when the
  CLI is found.
- **API key** — calls the Anthropic API directly. The key is stored in the
  macOS Keychain (`secrets.rs`), never in the repo or the config file.

Either way the conversation is read-only with respect to Sleeper: Claude is
shown the current board or matchup and never writes anything back.
