# Draft Assistant

Local-first fantasy football draft assistant for Sleeper leagues. You draft in
Sleeper as normal; this app is a read-only second screen that polls the public
Sleeper API, tracks every pick live, and tells you who to take under **your
league's exact scoring rules**.

Built with Tauri 2: Rust core engine + React/TypeScript-strict frontend.
Desktop (macOS) now; the same core compiles into an Android build later — no
server anywhere.

## What it does

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

## Run (dev)

```bash
bun install
bun run tauri dev
```

The project uses **Bun** as the package manager and script runner (Vite and
Vitest run under it unchanged). `npm` still works if you prefer it, but the
lockfile checked in is `bun.lock`.

## Build (release .app)

```bash
bun run tauri build
# → src-tauri/target/release/bundle/macos/Draft Assistant.app
```

## Testing

```bash
bun run verify        # everything: LOC cap, fmt, typecheck, build, tests, e2e, lint
bun run test          # Vitest (frontend) + cargo test (Rust)
bun run test:e2e      # Playwright against the browser preview
```

- **Rust — 55 tests.** Unit tests per module, fixture-driven integration tests
  (`src-tauri/tests/`), a 210-pick draft simulation with invariant checks,
  Sleeper wire-format parsing tests, and **property-based tests** (`proptest`)
  over the draft math and every deserializer.
- **Frontend — 15 Vitest tests** in jsdom, plus **7 Playwright tests** driving a
  real Chromium against the browser-preview fixture.
- **Fuzzing** — three `cargo-fuzz` targets in `src-tauri/fuzz/`. They build but
  do not currently run on macOS 27; see `src-tauri/fuzz/README.md` for why and
  what covers the gap.

Playwright covers the rendered UI, not the Tauri IPC boundary (the browser
fallback stubs it). A true desktop E2E on macOS would need WebdriverIO's
embedded WebDriver server — `tauri-driver` has no macOS WKWebView driver.

## Headless state dump / simulation

```bash
cd src-tauri
cargo run --bin dump_state -- <league_id> [sleeper_username] [out.json] [--simulate N]
```

`--simulate N` fakes the first N picks (market drafts by ADP, your slots take
the balanced recommendation) to exercise mid-draft state without a live draft.

## Ask Claude

The **Ask Claude** button opens a chat panel that answers questions about the
live draft — "who should I take next?", "who is likely gone before my next
pick?". It sends the current state (your roster, the top 40 of the board with
VORP/survival/tier, tier alerts, and the app's own recommendation) and returns
a short answer. Expect ~5-10 seconds per question.

It works by shelling out to the locally installed [Claude
Code](https://claude.com/claude-code) CLI, so it needs no API key — it uses
whatever that CLI is already logged in as. The panel is read-only advice: it
cannot draft, and `--restricted` strips the CLI's command- and code-running
tools.

If the CLI is not on `PATH` (notably inside a packaged `.app`, which gets a
minimal environment), the app looks in `~/.local/bin`, `/opt/homebrew/bin`, and
`/usr/local/bin`. Override with:

```bash
export DRAFT_ASSISTANT_CLAUDE_BIN=/full/path/to/claude
```

Errors surface in the panel rather than being swallowed — a missing CLI names
the env var above, and a login failure shows the CLI's own stderr.

## Browser preview

The UI degrades to a read-only preview when opened in a plain browser (vite dev
server on :1420): it renders `public/dev-fixture.json`, a captured state dump.
Regenerate the fixture with `dump_state`.

## Draft-day troubleshooting

Every failure the app can detect is shown on screen. This is what each one
means and what to do about it. "Pill" is the live-sync button in the header;
"warning" is the amber banner under the clock; "toast" is the message at the
bottom of the window — failures stay until dismissed, confirmations fade.

| You see | What it means | What to do |
|---|---|---|
| Pill **● Sync retrying** | One poll of Sleeper failed. | Nothing yet — the next poll is in 3 s. |
| Pill **● Sync stale · N failures** (red) | N polls in a row failed; the board is frozen at the last good state. Hover the pill for the error. | Check the network. Picks made in Sleeper meanwhile catch up on the next good poll. If it stays red, record picks by hand with the **Draft** buttons — Sleeper's picks override them the moment sync recovers. |
| Pill **○ Live sync off** | Polling is switched off. | Click the pill. |
| Warning `players refresh failed; using cache aged Nh` / `projections refresh failed; using cache aged Nh` / `weekly projections refresh failed; using cache aged Nh` | Sleeper did not answer; the app is running on its last download. Rankings are as good as that download. | Keep drafting. Click **Refresh data** once the network is back. |
| Warning `weekly projections unavailable for weeks …` | Some per-week projections were missing. Only the yardage-bonus estimate loses a little precision. | Ignore. |
| Warning `<file> could not be cached (…); will refetch` | The download worked but could not be saved — usually disk space or permissions. | Free disk space. The app works; it re-downloads on next launch. |
| Warning `board unusually small (N players) — projections may be incomplete` | Sleeper's projections endpoint returned a partial list. | **Refresh data**. If it persists the endpoint is degraded: rankings below the top ~100 are unreliable, lean on ADP and survival. |
| Warning `initial picks refresh failed: …` | The pick list did not load at startup. | Live sync retries every 3 s and clears it. |
| Warning `your draft slot N is outside the valid range …` / `draft reports 0 teams …` | Sleeper sent malformed draft settings; the app clamped them. | **Refresh data**. If it persists, the draft page in Sleeper is the source of truth. |
| Warning `mock draft: league settings synthesized …` | You loaded a mock draft by its draft ID; scoring is Sleeper's default for that mock type. | Informational. |
| Toast `player already drafted` / `draft is complete` / `no manual picks to undo` | A manual pick or undo was refused because the state already says so. | Dismiss it. |
| Toast `Live update rejected: Incompatible draft data …` | The backend and the UI disagree on the state format — a stale build. | Quit and relaunch. In dev, rebuild with `bun run tauri dev`. |
| Toast `write …/config.json.tmp: …` / `replace config.json: …` | Your league and username could not be saved. | Free disk space or fix permissions. This session keeps working; the next launch would show Setup. |
| Ask Claude `could not run the Claude CLI at … set DRAFT_ASSISTANT_CLAUDE_BIN` | The `claude` binary was not found. | `which claude`, then `export DRAFT_ASSISTANT_CLAUDE_BIN=<that path>` and relaunch. |
| Ask Claude `Claude CLI error: …` | The CLI ran and failed — usually not logged in. | Run `claude` in a terminal and complete the login. |
| Ask Claude `Claude did not answer within 45s` / `returned an empty answer` | Slow or hung model call. | Ask again, or **Cancel** and carry on — the board never waits on the chat. |
| A page saying **Draft Assistant hit a display error** | The screen crashed rendering the state. The engine is still polling. | **Reload state**. If it recurs, **Restart app** — API picks and manual picks are all on disk. |

### Resetting local state

Everything lives in `~/Library/Application Support/com.justin.draft-assistant/`.
Quit the app before deleting, relaunch after.

| Delete | You lose | You get |
|---|---|---|
| `manual_picks_<draft>.json` | Manual fallback picks for that draft (API picks are unaffected) | A board that trusts Sleeper only |
| `players.json`, `projections_*.json`, `weekly_*.json` | Cached downloads | A forced re-download on next launch (~10 s) |
| `config.json` | Saved league and username | The Setup screen |
| `draft-state.json` | The last **Export state** file | Nothing — regenerated on export |

There is no log file yet. In dev, backend output appears in the terminal
running `bun run tauri dev`.

## Data sources

- `api.sleeper.app/v1` — league, draft, picks, players (documented, no auth)
- `api.sleeper.app/projections/nfl/{season}[/{week}]` — raw-stat projections +
  ADP (undocumented; responses are cached and parsed defensively)

Caches live in the app data dir (`~/Library/Application Support/
com.justin.draft-assistant/`): players 24h TTL, projections 6h TTL.
"Refresh data" forces a refetch and board rebuild.

## Layout

```
src/                 React + TS strict UI (App.tsx, api.ts, types.ts)
src-tauri/src/
  sleeper.rs         API client + response types
  scoring.rs         data-driven scorer + per-game bonus model
  valuation.rs       replacement levels, VORP, tiers
  draft.rs           snake math, rosters, survival probabilities
  board.rs           scored board assembly (incl. bye inference)
  view.rs            DraftView: the one state struct (UI + AI dump)
  recommend.rs       deterministic recommendation modes
  engine.rs          caching, config, league loading
  lib.rs             Tauri commands + 3s pick poller
  bin/dump_state.rs  headless dump + draft simulator
```

All files ≤ 500 LOC by project convention.
