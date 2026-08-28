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
- **Keeper leagues.** Keepers arrive as picks already in the book at scattered
  pick numbers, before the draft starts. The clock follows the lowest *unfilled*
  pick, your remaining picks skip the ones a keeper used, and keepers the draft
  has not reached stay out of "recent picks".
- **Live draft tracking.** Polls `GET /draft/{id}/picks` every 3s; on-the-clock
  banner with the pick clock (`last_picked + pick_timer`), all 14 rosters,
  tier alerts, position-run detection, recent picks by manager name. Snake,
  linear, and third-round-reversal orders are modelled from the draft payload.
- **Survival odds.** P(player lasts to your next pick) from ADP, *conditioned
  on the player still being on the board now* — a faller 30 picks past his
  ADP is judged from this pick, not from pick 1 — with a sigma that widens the
  further the market has already been wrong. The ADP column follows the
  league: PPR / half / standard, or the 2-QB market for superflex leagues.
- **Recommendations with reasons.** Deterministic `balanced` and `safe` modes;
  every suggestion lists its auditable reasons (VORP, roster need, tier
  scarcity, survival, ADP value).
- **Manual pick fallback + undo** if the API lags or the draft is offline.
  API picks always win over manual ones; Undo is only enabled while a manual
  pick exists.
- **AI-readable by design.** One call (`get_state` command, "Export state"
  button, or the `dump_state` CLI) emits the entire draft state as JSON — the
  same struct the UI renders. Point an LLM at it; nothing needs to scrape the UI.
- **Ask Claude, in context.** A chat panel that sees the whole board, every
  roster, recent picks and the app's recommendation, remembers the
  conversation, and lets you pick model, thinking effort, speed, and whether
  it may search the web. See [Ask Claude](#ask-claude).
- **Sortable board.** Click any column header — Bye, Pos, Pts, VORP, Tier,
  ADP, Surv — to sort; click again to flip; `#` restores value order. `/` jumps
  to the search box, and "Show all" lifts the 200-row cap.
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

The **Ask Claude** button opens a panel beside the board that answers
questions about the live draft — "who should I take next?", "who is likely
gone before my next pick?", "why?". Each question is sent with:

- the current draft state (clock, your roster, all rosters, tier alerts,
  recent picks, the app's own recommendations, replacement baselines);
- the **entire** available board as a compact table — rank, name, position,
  team, bye, points, VORP, tier, ADP, survival odds, injury tag — so it can
  answer about QB2s, TE4s and every DEF, not just the top 40;
- the conversation so far (the last six exchanges, plus any summary), so
  follow-ups like "what about the RB instead?" mean something.

Expect 15–40 seconds per answer with Opus (a measured example: 36 s and
~42k context tokens for a three-position plan); the panel offers **Cancel**
while it waits and never blocks the board.

**Settings** (folded at the top of the panel, remembered between launches):

| Setting | Choices | Notes |
|---|---|---|
| Model | Opus (default), Sonnet, Fable | Sonnet is quicker and cheaper; Fable is the strongest and slowest. `DRAFT_ASSISTANT_CLAUDE_MODEL` sets the default. |
| Thinking effort | Default, Low … Max | Passed as `--effort`. Low is enough for lookups; Max for "plan my next three picks". |
| Fast mode | off / on | Asks for the CLI's fast mode. If the account cannot serve it the panel says so once (e.g. `extra_usage_disabled`) and answers at standard speed. |
| Web search | off (default) / on | Lets the model search for injury details, holdouts or depth-chart news. It says when an answer relies on the web; rankings still come from the board. Slower. |

Under the thread a usage line shows the last answer's context size in
tokens, its duration, the model, the question count and the session cost as
the CLI reports it. **New chat** starts an empty thread. **Compact** folds a
long thread into a short summary that then stands in for the earlier turns —
it is one extra model call and can take a minute or two, so it is only
enabled once there are at least two questions.

It works by shelling out to the locally installed [Claude
Code](https://claude.com/claude-code) CLI, so it needs no API key — it uses
whatever that CLI is already logged in as. The panel is read-only advice: it
cannot draft. The CLI runs with `--restricted` and `--tools ""` (or `--tools
WebSearch` when web search is on), so it has no command, code or file tools.

If the CLI is not on `PATH` (notably inside a packaged `.app`, which gets a
minimal environment), the app looks in `~/.local/bin`, `/opt/homebrew/bin`, and
`/usr/local/bin`. Override with:

```bash
export DRAFT_ASSISTANT_CLAUDE_BIN=/full/path/to/claude
```

Errors surface in the panel rather than being swallowed — a missing CLI names
the env var above, and a login failure shows the CLI's own stderr.

## Demo / replay mode

`scripts/replay-sleeper.mjs` stands in for `api.sleeper.app` on a local port
and replays a **completed** draft as if it were live: the league, draft, and
picks endpoints for that draft come from a recording, with one pick released
every few seconds; players, projections, and users are proxied to the real
API. Any completed draft works — last season's draft of your own league is the
most realistic, since the team count, rounds, and users match.

```bash
# 1. build the headless engine once
cargo build --manifest-path src-tauri/Cargo.toml --bin dump_state

# 2. replay last year's draft with you at slot 2, one pick every 8 s, and
#    write a fresh state dump for the browser preview every 4 s
bun scripts/replay-sleeper.mjs --league <last_season_league_id> --draft <its_draft_id> \
  --season 2026 --my-user <your_user_id> --my-slot 2 --interval 8 \
  --dump public/live-state.json --username <your_username>

# 3a. watch the real UI follow it in a browser
bun run dev            # then open http://localhost:1420/?replay=/live-state.json

# 3b. or run the desktop app against it
DRAFT_ASSISTANT_SLEEPER_BASE=http://localhost:8787 bun run tauri dev
```

While it runs: `curl localhost:8787/replay/status`, `/replay/step`,
`/replay/pause`, `/replay/resume`, `/replay/set?n=27`. The server log shows
every poll the app makes, so sync cadence is visible. `public/live-state.json`
is git-ignored.

`DRAFT_ASSISTANT_SLEEPER_BASE` redirects every request the engine makes, so
it also serves as the seam for offline tests against a recorded API.

## Browser preview

The UI degrades to a read-only preview when opened in a plain browser (vite dev
server on :1420): it renders `public/dev-fixture.json`, a captured state dump.
Regenerate the fixture with `dump_state`. Add `?replay=<url>` to poll a state
dump that keeps changing — see [Demo / replay mode](#demo--replay-mode).

## Before the draft

Twenty minutes of setup on a good connection saves the two failures most
likely to bite on draft night.

1. **Launch the app and look at it.** `bun run tauri dev` (or the packaged
   `.app`). Confirm the league name, your slot, and — in a keeper league — that
   your keepers are on your roster and missing from "Your picks".
2. **Click Refresh data while you are still on good wifi.** Projections are
   cached for 6 hours and the player dictionary for 24, so a draft that starts
   more than six hours after your last launch will re-download ~20 MB at the
   worst possible moment. Refreshing beforehand makes the venue's wifi
   irrelevant. (If it does fail there, the app falls back to the cached copy and
   says so in an amber banner — you can draft on it.)
3. **Check the sync pill goes green.** "● Live sync on" with a "Last sync Ns
   ago" that keeps counting. If it says "Sync stale · nothing for …", nothing is
   arriving.
4. **Ask Claude one throwaway question** if you plan to use it, so you find out
   then — not at pick 27 — whether the CLI is logged in.
5. **Know the fallback.** If Sleeper's API stalls, the **Draft** button on each
   row records the pick locally; the board keeps working and Sleeper's own picks
   override yours the moment sync recovers. **Undo** removes the last manual
   pick (it is disabled when there is nothing to undo).

Two keystrokes worth remembering: **`/`** jumps to the player search, **Esc**
closes the confirm dialog or the chat panel.

## Draft-day troubleshooting

Every failure the app can detect is shown on screen. This is what each one
means and what to do about it. "Pill" is the live-sync button in the header;
"warning" is the amber banner under the clock; "toast" is a message —
failures stay as a red bar under the header until dismissed, confirmations
fade from the top-right corner.

| You see | What it means | What to do |
|---|---|---|
| Pill **● Sync retrying** | One poll of Sleeper failed. | Nothing yet — the next poll is in 3 s. |
| Pill **● Sync stale · N failures** (red) | N polls in a row failed; the board is frozen at the last good state. Hover the pill for the error. | Check the network. Picks made in Sleeper meanwhile catch up on the next good poll. If it stays red, record picks by hand with the **Draft** buttons — Sleeper's picks override them the moment sync recovers. |
| Pill **○ Live sync off** | Polling is switched off. | Click the pill. |
| Pill **● Sync stale · nothing for Nm** | Polls are not failing, but nothing has arrived for over 30 s. The draft may simply be quiet — or the feed has stopped. | Watch the pick number. If Sleeper has moved on and this has not, toggle the pill off and on. |
| Warning `players refresh failed; using cache aged Nh` / `projections refresh failed; using cache aged Nh` / `weekly projections refresh failed; using cache aged Nh` | Sleeper did not answer; the app is running on its last download. Rankings are as good as that download. | Keep drafting. Click **Refresh data** once the network is back. |
| Warning `weekly projections unavailable for weeks …` | Some per-week projections were missing. Only the yardage-bonus estimate loses a little precision. | Ignore. |
| Warning `<file> could not be cached (…); will refetch` | The download worked but could not be saved — usually disk space or permissions. (Before 2026-08-28 two overlapping loads could also trigger this on `weekly_*.json`; that race is fixed.) | Free disk space. The app works; it re-downloads on next launch. |
| Warning `board unusually small (N players) — projections may be incomplete` | Sleeper's projections endpoint returned a partial list. | **Refresh data**. If it persists the endpoint is degraded: rankings below the top ~100 are unreliable, lean on ADP and survival. |
| Warning `initial picks refresh failed: …` | The pick list did not load at startup. | Live sync retries every 3 s and clears it. |
| Warning `your draft slot N is outside the valid range …` / `draft reports 0 teams …` | Sleeper sent malformed draft settings; the app clamped them. | **Refresh data**. If it persists, the draft page in Sleeper is the source of truth. |
| Warning `mock draft: league settings synthesized …` | You loaded a mock draft by its draft ID; scoring is Sleeper's default for that mock type. | Informational. |
| Warning `draft type 'auction' is not supported; pick order is modelled as a snake` | Sleeper reports a draft type the app cannot model. Snake, linear, and third-round-reversal are handled. | The board and rosters are still right; ignore the on-clock slot and "your picks". |
| No **CLOCK** cell in the banner | The draft has no pick timer, has not started, or is complete. It appears once the draft is `drafting` and Sleeper has stamped `last_picked`. | Nothing. If Sleeper shows a timer and the app does not, the poll is stale — check the pill. |
| **Undo** greyed out | There is no manual pick to undo. Sleeper's own picks cannot be undone here. | Nothing. |
| Toast `player already drafted` / `draft is complete` | A manual pick was refused because the state already says so. | Dismiss it. |
| Setup `request failed: …/v1/user/<name>: … operation timed out` / `connection refused` | The username lookup could not reach Sleeper (8 s limit). | Check the network and try again, or leave the username blank and add it later. |
| Toast `Live update rejected: Incompatible draft data …` | The backend and the UI disagree on the state format — a stale build. | Quit and relaunch. In dev, rebuild with `bun run tauri dev`. |
| Toast `write …/config.json.<pid>.<n>.tmp: …` / `replace config.json: …` | Your league and username could not be saved. | Free disk space or fix permissions. This session keeps working; the next launch would show Setup. |
| Ask Claude `could not run the Claude CLI at … set DRAFT_ASSISTANT_CLAUDE_BIN` | The `claude` binary was not found. | `which claude`, then `export DRAFT_ASSISTANT_CLAUDE_BIN=<that path>` and relaunch. |
| Ask Claude `Claude CLI error: …` | The CLI ran and failed — usually not logged in. | Run `claude` in a terminal and complete the login. |
| Ask Claude `Claude did not answer within 90s` (150 s with web search, 180 s for Compact) / `returned an empty answer` | Slow or hung model call. | Ask again — lower the thinking effort or switch to Sonnet if it keeps happening — or **Cancel** and carry on; the board never waits on the chat. |
| Ask Claude `unexpected Claude CLI output (…)` | The CLI printed something other than its JSON result — usually a CLI update changed the format. | Run `claude --version`; report it. Answers are unavailable until the format is handled. |
| Ask Claude `unknown model '…'` / `unknown effort '…'` | `DRAFT_ASSISTANT_CLAUDE_MODEL` names something the panel does not know. | Use `opus`, `sonnet`, `fable`, or `haiku`, or unset it. |
| Ask Claude note `Fast mode unavailable (extra_usage_disabled) — answered at standard speed.` | Fast mode was requested but the account cannot serve it. | Answers still arrive; turn the setting off to silence the note, or enable extra usage on the account. |
| Ask Claude `Nothing to compact yet` | Compact was pressed on a thread that is already just a summary. | Ask more questions first. |
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
