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

The season dump reads through the same on-disk cache as the app, so delete
`/tmp/draft-assistant-cli` first if you want genuinely fresh data.

## Data sources

- `api.sleeper.app/v1` — league, draft, picks, players (documented, no auth)
- `api.sleeper.app/projections/nfl/{season}[/{week}]` — raw-stat projections +
  ADP (undocumented; responses are cached and parsed defensively)

Caches live in the app data dir (`~/Library/Application Support/
com.justin.draft-assistant/`): players 24h TTL, projections 6h TTL.
"Refresh data" forces a refetch and board rebuild.

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
    engine.rs               caching, config, league loading
    projections.rs          projection fetch + stale-cache fallback
    roster.rs               slot rules (flex eligibility, draftable positions)
    scoring.rs              data-driven scorer + per-game bonus model
    headshots.rs            on-disk image cache (players + manager avatars)
    secrets.rs              API key in the macOS Keychain
    state.rs                Tauri-managed app state
  draft screen
    draft.rs                snake math, rosters, survival probabilities
    board.rs                scored board assembly (incl. bye inference)
    valuation.rs            replacement levels, VORP, tiers
    recommend.rs            deterministic recommendation modes
    view.rs                 DraftView: the one state struct (UI + AI dump)
    simulation.rs           deterministic draft simulation
  season screen
    season.rs               build_season_view + SeasonAnalysis
    season_view_parts.rs    lookup, current lineup, start/sit reasons
    season_api.rs           season-only Sleeper endpoints + DTOs
    season_engine.rs        season load, week sweep, live refresh
    season_lineup.rs        optimal lineup solver
    season_odds.rs          playoff-odds simulation
    season_moves.rs         waiver targets by marginal lineup gain
    season_trades.rs        trade ideas; season_deals.rs completed trades
    season_activity.rs      league activity feed
    season_live.rs          live scoreboard; season_history.rs trends
    weekly.rs               weekly projection lookup
  ask claude
    chat.rs                 Anthropic Messages API route
    chat_cli.rs             Claude Code CLI route
    chat_context.rs         what Claude is shown per screen
  commands_draft.rs         draft commands + 3s pick poller
  commands_season.rs        season commands + 30s live poller
  commands_chat.rs          chat commands
  lib.rs                    command registration
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
