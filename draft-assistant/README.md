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
screen ([docs/replay.md](docs/replay.md) makes the preview move instead):

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
  main.tsx                  mounts App, plus the picture-zoom layer
  App.tsx                   screen switching, league setup, the draft poller,
                            shared state
  api.ts                    the IPC surface (+ browser-fixture fallback), and
                            the schema-version check on every payload
  session.ts                the season screen's data lifecycle: first load,
                            starting and stopping the live poller, retry
  boardIdentity.ts          keeps the available-players array identity stable
                            when an update did not actually change it, so the
                            board does not re-sort for nothing
  types.ts                  mirrors the Rust DraftView structs
  season-types.ts           mirrors the Rust SeasonView structs
  chat-types.ts             mirrors the Rust chat structs
  format.ts                 shared display helpers (numbers, percents, clocks)
  theme.ts                  light/dark: follow the OS, remember an override
  prefs.ts                  small shared settings, e.g. the on-the-clock chime
  avatars.ts                Headshots/Team-logos choice + per-session picture
                            cache in front of the backend's disk cache
  zoom.ts                   whichever picture is currently shown large
  components/
    lazyScreens.tsx         the code-split boundary: each screen and the chat
                            become their own chunk, fetched when first shown
    ErrorBoundary.tsx       catches a screen that fails to render, so one bad
                            chunk does not blank the whole window
    Header.tsx              league identity, Draft/Season toggle, sync badge,
                            settings menu
    DraftScreen.tsx         the draft cockpit, assembled from the parts below
    ClockBanner.tsx         round, pick, who is on the clock, the pick queue
    Panels.tsx              the three recommendation cards and the left rail
    Board.tsx               the player board: sortable, filterable
    SeasonScreen.tsx        the in-season screen: header stats, main column,
                            tabbed rail
    ThisWeek.tsx            start/sit calls, head-to-head lineup, waivers
    SeasonTabs.tsx          the right rail: Standings, Games, My team, League,
                            Last season
    GamesTab.tsx            live NFL scoreboard joined to both sides' starters
    TrendsTab.tsx           every team's strength over time + why it moved
    Chat.tsx                the Ask Claude panel: pickers, thread, composer
    Overlays.tsx            modal confirm and the toast strip
    useFocusTrap.ts         the one focus trap, shared by everything modal
    bits.tsx                the visual primitives both screens share
  *.css                     ten stylesheets, one per area; `check:css` refuses
                            to let a class be styled from more than one
  *.test.ts / *.test.tsx    vitest, beside the file each one tests
  test/                     the vitest setup and its async settle helper
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

## Testing

`npm run verify` is the gate: format, lint, typecheck, the vitest suite, the
Rust suite, and a production `vite build`. Everything in it runs offline and
finishes in seconds, and it is meant to stay that way.

### What is not covered

Nothing in `verify` launches the app. The React bundle is tested in jsdom, the
Rust is tested by calling functions directly, and the two never meet — so the
seam where they do meet in production is the one thing no test watches: the
command names the frontend types into `invoke()`, the capability set, and the
CSP the built bundle has to load under.

`src-tauri/tests/command_surface.rs` closes the first of those three. It stands
the app up on Tauri's mock runtime with the same state `lib.rs` installs, sends
each command a real IPC message, and fails if the dispatcher does not recognise
the name. It also reads `lib.rs` and asserts that the `generate_handler!` list
is exactly the set of `#[tauri::command]` functions in the crate, which is the
one failure that otherwise reaches the user: a command written, wired up in
`api.ts`, and never registered. Two commands take a bare `tauri::AppHandle`
(i.e. `AppHandle<Wry>`) and so cannot be registered on the mock runtime;
they are covered by the source-level check but not the IPC round trip.

The capability set and the CSP are covered by `npm run test:e2e` below, a real
window rather than a mock — not in `verify`, so between runs the manual check
stands in. `npm run test:e2e:browser` drives the preview: [docs/replay.md](docs/replay.md).

### Manual smoke check

Sixty seconds, and it sees more than the automated run does. Do this after
touching `tauri.conf.json`, `capabilities/`, `lib.rs`, or the Vite chunking;
`npm run test:e2e` covers item 1 and half of item 2, and nothing else here:

```bash
npm run tauri dev
```

1. The window opens and is not blank. A blank window with content in the DOM
   means the CSP rejected a chunk — check the WKWebView console.
2. The saved league restores, or the setup screen offers to add one. Either way
   the launch screen resolves; if it hangs, `get_config` did not answer.
3. The draft board renders rows, and picking a player opens the confirm dialog.
4. The season screen opens and shows the matchup, lineup and standings.
5. Player headshots and manager avatars render. They arrive as `data:` URLs, so
   a missing image is usually a change to `img-src` in the CSP.
6. Ask Claude opens and reports a provider.

### End-to-end, for real

```bash
npm run test:e2e
```

One test. It launches the built app, waits for the launch screen to resolve,
and asserts a real screen is on it — the header with the league name and the
Draft/Season toggle if a league is configured, the setup form if not.
Deliberately narrow: it is the only test here that loads the built bundle
into a WKWebView under the production CSP and sends an `invoke()` over the
real IPC bridge, so it aims at what that seam produces — a window that opens
blank, or never gets past "Restoring…".

Real, not headless: the session reports as `webkit 605.1.15 macos`, and on a
machine with a league saved the spec logs what it read off the screen:

```
[e2e] resolved on .app-header; screen reads:
UMass Wrestling Fantasy Football LeagueWeek 1 · 0–0 · 7th of 14SeasonDraft
14-team full-PPR · 15 roundsLive · 0s agoAsk ClaudeThis weekvs Meatball ·
127.6 – 125.4Win odds52%Playoffs44%Locks in10d 18h…
```

The script builds with `tauri build --features wdio --no-bundle` into
`src-tauri/target/wdio/`, then points WebdriverIO at the binary. About a
minute of build and a minute of run, warm.

It has to be the **Tauri CLI, not `cargo build --release`**:
`generate_context!` embeds `dist/` only when `tauri/custom-protocol` is on
and the CLI is what turns it on, so plain cargo aims the webview at the
`devUrl` instead. That fails quietly — blank unless a Vite dev server happens
to be up, green against the dev server rather than the built bundle if one
is. The script gets it right; the trap is for driving the binary by hand.

#### How it works on macOS, and why that is safe

`tauri-driver` — the usual answer — is Windows and Linux only; its own README
lists macOS as Todo, and there is no WKWebView driver binary to point it at.
`@wdio/tauri-service` gets around this with its default `embedded` provider:
the WebDriver server runs *inside* the app, from the
`tauri-plugin-wdio-webdriver` crate. Nothing external to install.

That is a full remote-control surface, so it sits behind a cargo feature that
is off by default — in three places, all switched by the same flag:

- **The plugins.** `optional = true` in `Cargo.toml`, pulled in only by
  `[features] wdio`. Absent from the dependency graph, not merely unused.
- **The registration.** The two `.plugin(...)` calls in `lib.rs` are under
  `#[cfg(feature = "wdio")]`.
- **The permission.** `capabilities/wdio.json` grants `wdio:default`, and
  `build.rs` only feeds it to `tauri-build` when `CARGO_FEATURE_WDIO` is set.
  This one needs a build script rather than a `cfg`: a capability is baked
  into the ACL at compile time and cannot be revoked at runtime, so a grant
  left in `default.json` would ship a permission for a plugin that is not
  there — and re-arm silently the moment anyone added it back.

Tauri's ACL makes that last one self-enforcing rather than a thing to
remember. Feed `capabilities/wdio.json` to a build without the feature and it
does not warn, it refuses:

```
failed to run tauri-build: Permission wdio:default not found,
expected one of core:default, core:app:default, …
```

The evidence that a default build is clean:

```
$ cargo tree -e normal | grep -iE 'wdio|axum'                     # nothing
$ cargo tree -e normal --features wdio | grep -iE 'wdio|axum'
├── axum v0.8.9 … ├── tauri-plugin-wdio … └── tauri-plugin-wdio-webdriver
$ strings -a target/release/draft-assistant      | grep -ci wdio  # 0
$ strings -a target/wdio/release/draft-assistant | grep -ci wdio  # 48
$ nm -a target/debug/draft-assistant             | grep -ci wdio  # 0
$ nm -a target/wdio/debug/draft-assistant        | grep -ci wdio  # 3228
```

That build also goes to `src-tauri/target/wdio/`, so the binary with a
WebDriver server in it never sits where `npm run tauri build` writes.

The service also offers `browser.tauri.execute()` and command mocking, and
this uses neither: both need `withGlobalTauri: true` plus an
`@wdio/tauri-plugin` import in the app's own entry point — a global
`__TAURI__` on `window`, and test code inside the shipped `index-*.js`. Not
worth adding to production for a smoke test, so the run drives plain
WebDriver and the service logs `Tauri core.invoke not available` every few
seconds while probing for what is not there. Expected noise; still green.

#### Why it is not in `verify`, and where it is instead

Its own workflow, `.github/workflows/e2e.yml` — on pushes to `main`, on PRs
touching the seam it watches (`lib.rs`, `capabilities/`, `build.rs`,
`tauri.conf.json`, `vite.config.ts`, `e2e/`), and on `workflow_dispatch`.

It stays out of the PR gate because it is expensive and can fail for reasons
that are not the diff's fault: `--features wdio` is a different feature set
from everything `verify` builds and shares no artifacts with it — a second
full compile of `tauri`, `wry` and `reqwest` plus `axum` and the plugins, on
a runner billed at ten times the Linux rate — and it needs a real window and
the live Sleeper API. Its npm side is another ~152 MB.

Which is why `e2e/` is **its own npm package with its own lockfile** rather
than devDependencies of the app. Everyday `npm ci` stays at 209 MB instead of
324 MB — and, the sharper reason, WebdriverIO's tree carries high-severity
advisories with no fix available (`deepmerge-ts` and everything above it), so
keeping it out means `npm audit --audit-level=high` in the `audit` job still
reports `found 0 vulnerabilities` rather than being turned down to
accommodate a test harness.

For the same reason `npm run lint:rust` is on default features rather than
`--all-features`: asking `verify` for both feature sets would double its Rust
compile. The `#[cfg(feature = "wdio")]` path is linted at the same
`-D warnings` by `npm run lint:rust:wdio`, in the e2e job that already pays
for that build.
