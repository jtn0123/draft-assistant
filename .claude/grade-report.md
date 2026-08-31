# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-30 (evening — third pass, fresh eyes; supersedes the same-day
first pass, archived as `grade-report-2026-08-30-first-pass.md`)
**Stack:** Tauri 2 desktop app — Rust/Tokio backend + React 19 / TypeScript-strict / Vite frontend, ~19k LOC first-party

## Summary

This is a deeper audit than the morning pass: three explorers re-read the tree
after the day's 12 commits, ran per-file coverage, and traced concurrency
interleavings. The app got much better today **and** this audit holds it to a
higher bar — the two are not in tension. Several findings below are real bugs
that survived every prior round.

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B+ | 4 |
| B | Backend Quality | B | 7 |
| C | Frontend Quality | B | 7 |
| D | Testing & Reliability | B | 8 |
| E | Security | A− | 3 |
| F | Dependencies & Tech Currency | A− | 2 |
| G | Performance & Scalability | B | 8 |
| H | Documentation & Onboarding | B− | 4 |
| I | Developer Experience & Tooling | B+ | 4 |
| **Overall** | | **B+** | **47** |

**Top 5 highest-leverage fixes:** C1, B1, B2, D1, D2 — all done.

### Executed 2026-08-30 evening — 18 of 47 items

The user asked for the top 15; 18 landed (D3, A3 and I4 fell out of their
neighbours). One commit each, `5a7245e`..`81d1327`. Done: **A2, A3, B1, B2, B3,
B4, C1, C2, D1, D2, D3, D4, G1, G2, G3, I1, I3, I4.**

Three findings were sharpened by the work rather than merely applied:

- **B2 was proved, not assumed.** A test with 120 mediocre free agents ranked
  ahead of a 25-pt/wk streamer fails on the old code (the streamer is invisible)
  and passes after. The bug was real.
- **G1's premise was half wrong, and the agent caught it.** The activity feed
  also computes empty-lineup-slot warnings from `rosters`, which *do* refresh
  live. Caching the whole feed would have frozen "your RB slot is empty" for ten
  minutes. Transactions are cached; the empty-slot check still runs every tick.
- **I1 found a test-suite blind spot.** Type-aware lint revealed every mocked
  `askClaude` reply in `Chat.test.tsx` was missing four fields the backend
  actually sends — a shape no coverage percentage would have flagged.

Post-execution baselines: Rust **76.56%** lines / **245** tests (floor raised
68 → 74); frontend **89.42%** lines / **173** tests, now genuinely gated;
entry CSS 35 → 15 kB; pre-commit runs tests in ~11s. `npm run verify` green.

### Second execution wave — 2026-08-30 late evening

20 more items landed (`32aab18`..`b0a5aaf`), taking the total to **38 of 47**:
**B5, B6, B7, C3, C4, C6, C7, E1, E2, E3, F1, F2, G4, G5, G6, G8, H1, H2, H3, H4.**

Worth recording:

- **E1 was worse than written.** `rustsec/audit-check@v2` — the action handed
  `GITHUB_TOKEN` — is not a tag at all but a **branch**, mutable by anyone with
  upstream write access. All six actions are now pinned to resolved SHAs.
- **F2's first measurement was wrong and the agent said so.** An apparent
  22s → 12s win was contention from three concurrent agents sharing a target
  dir. Re-measured in isolation: ~0.5s off a ~2.4s incremental rebuild. The real
  win is no longer writing a 366 MB `staticlib` on every build.
- **C3 needed care, not just an `aria-live`.** A live region on a per-second
  clock would re-announce every tick. The announced sentence is now held against
  a situation key (status / whose pick / pick number), so seconds are captured
  once at the transition and frozen; a test advances 5s and asserts the visible
  timer moved while the announcement did not.
- **G6 measured: 2.11 ms → 1.58 ms** per analysis rebuild (~25%), A/B in release.
  G4/G5/G8 are latency and runtime-blocking wins that were deliberately **not**
  given invented numbers.
- **Found and correctly left alone:** `season_engine::live_is_stale()` and
  `LIVE_TTL_SECS` have no callers anywhere; because the fn is `pub`, no
  dead-code warning fires. The 30s interval that actually drives the poller is
  set independently in `session.ts`. Two unrelated 30s numbers, one inert.
  Documented as a new item rather than silently deleted.

Post-wave baselines: Rust **77.56%** lines / **252** tests; frontend **~90%**
lines / **197** tests; `npm run verify` green.

**Remaining open (9):** A1, A4, C5, D5, D6, D7, D8, G7, I2.

Baselines at audit time: Rust 74.77% lines / 212 tests; frontend 88.97%
lines / 160 tests; `npm audit` and `cargo audit` clean; `npm run verify` green.

---

## A — Architecture & Design — B+

The big structural work landed: the season view is split into one module per
section, `Engine`'s seams are trait-declared, and the 500-LOC cap holds
everywhere. What remains is consistency debt: the Sleeper client's route list
is still split across two files with duplicated constants, and the whole crate
uses `Result<_, String>` (74 occurrences, 15 files) so callers can't tell a
terminal error from a retryable one without string-matching.

#### A1 — The Sleeper client is still defined in two files with duplicated base URLs
- **Where:** `src-tauri/src/season_api.rs:11-12,269-311` vs `sleeper.rs:12-13,254`
- **What's wrong:** `BASE`/`BASE_UNDOC` are byte-identical in both files, and six client methods live in `season_api.rs` as an inherent-impl extension of a type owned by `sleeper.rs`. Changing the Sleeper host means editing two files; "where are the HTTP methods" has two answers.
- **Fix:** Make the constants `pub(crate)` in `sleeper.rs` and import them; either move the six methods into `sleeper.rs` (splitting that file first) or define a `SeasonEndpoints` trait so the extension matches the `Engine` convention (named trait, indexed in the doc comment).
- **Effort:** M
- **Grade lift:** B+ → A− (closes the last layering inversion)

#### ~~A2~~ ✓ done 2026-08-30 — Stringly-typed errors everywhere: `Result<_, String>` is the only error type in the crate
- **Where:** 74 occurrences across 15 files; the retryable bit is computed and then discarded at `sleeper.rs:288`
- **What's wrong:** Callers cannot distinguish "league not found" (terminal) from "HTTP 503" (retryable) without matching message text. The transport layer already knows which is which and flattens it away.
- **Fix:** Introduce a small `SleeperError { NotFound, Http(StatusCode), Transport, Decode }` with `impl Display`; the Tauri boundary keeps serialising to a string. Convert the transport layer first; let the rest migrate opportunistically.
- **Effort:** M
- **Grade lift:** B+ → A− (retry/UX decisions become principled instead of textual)

#### ~~A3~~ ✓ done 2026-08-30 — Two different position resolvers can disagree about the same roster
- **Where:** `src-tauri/src/season_history.rs:62` (uses `player_meta` directly) vs `season_view_standings.rs:24` (uses `Lookup::position`, which prefers the board)
- **What's wrong:** When board and metadata disagree (board carries the league-scored position), Trends "strength" and the standings projection are computed from different lineups for the same team.
- **Fix:** Pass a `Lookup` into `take_snapshot` so both paths resolve positions identically.
- **Effort:** S
- **Grade lift:** B+ → B+ (consistency; removes a silent divergence)

#### A4 — `App.tsx` still hand-rolls persisted state the stores already know how to do
- **Where:** `src/App.tsx:43-49,71,77-85,106,373-377`; the localStorage try/catch pattern is re-implemented in 5 places (`prefs.ts`, `avatars.ts`, `theme.ts`, `ThisWeek.tsx:9-17`, `App.tsx`)
- **What's wrong:** Theme occupies three hooks in App.tsx though `theme.ts` owns the logic; `screen` duplicates the persistence pattern a fifth time; `chime` is read from the store only to be drilled back down as two of `Header`'s 16 props.
- **Fix:** Add a `persisted<T>(key, parse, fallback)` helper to `prefs.ts`; build `useScreen()`, `useThemePreference()`, `useLineupView()` on it; let `Header` subscribe to `prefs.ts` directly and drop `chime`/`onToggleChime` from its props.
- **Effort:** M
- **Grade lift:** B+ → B+ (App.tsx stops being the store of last resort)

---

## B — Backend Quality — B

Error handling discipline, retries, atomic writes, and input validation are all
in place, and lock ordering was checked and found consistent (`loaded → season
→ config` at all 12 multi-lock sites). But the deeper pass found real
concurrency and correctness issues in the most stateful paths: the chat
command stalls both pollers, a league switch mid-load can cross-contaminate
files, config saves can fail silently, and the waiver search structurally
misses the players waivers exist for.

#### ~~B1~~ ✓ done 2026-08-30 — Asking the chat a question stalls both pollers
- **Where:** `src-tauri/src/commands_chat.rs:154-171`
- **What's wrong:** `ask_claude` runs the full `build_season_view` (4,000-iteration Monte Carlo, ~1,600 lineup solves, the trade search) synchronously on the async runtime thread while holding all three mutexes. Every question freezes the 30s season poller and the 3s draft poller until it finishes.
- **Fix:** Reuse the poller's cached analysis (store the last `SeasonView` or `SeasonAnalysis` in `AppState` and hand it to `chat_context::season_context`); at minimum clone the three inputs, drop the guards, and run the build inside `tokio::task::spawn_blocking`.
- **Effort:** M
- **Grade lift:** B → B+ (removes the biggest interactive stall in the app)

#### ~~B2~~ ✓ done 2026-08-30 — The waiver search never sees the hot streamer it exists to find
- **Where:** `src-tauri/src/season_view_market.rs:31-41`, `season_moves.rs:101`
- **What's wrong:** The free-agent pool is materialised in board order — sorted by *season* rank — then truncated to `CANDIDATE_POOL` before weekly evaluation. A free agent with high weekly points but a low season rank (the breakout/streamer case) is never evaluated. The other 540+ `FreeAgent` structs are allocated and thrown away.
- **Fix:** Sort (or `select_nth_unstable_by`) free agents on `weekly_points` before truncating, and truncate in `season_view_market` so the discarded ones are never allocated. Add a test: a low-season-rank, high-week player must appear as a target.
- **Effort:** S
- **Grade lift:** B → B+ (a user-visible product-correctness fix)

#### ~~B3~~ ✓ done 2026-08-30 — League switch during a season load cross-contaminates state
- **Where:** `src-tauri/src/commands_season.rs:20-35`; history write at `season_history.rs:147`
- **What's wrong:** `load_season` clones the league, drops the lock, awaits a multi-second network load, then re-locks and records history against whatever `loaded` holds *now*. If `add_league` ran in the gap, league A's roster snapshot is written into league B's history file and `state.season` holds league A's data under league B.
- **Fix:** Re-check `loaded.league.league_id` after the await and return `Err("league changed during load")`, or add a generation counter like the pollers already use.
- **Effort:** S
- **Grade lift:** B → B (removes a data-corruption path)

#### ~~B4~~ ✓ done 2026-08-30 — `save_config` discards every failure; the league list can silently vanish
- **Where:** `src-tauri/src/engine.rs:224-243`; callers at `commands_draft.rs:50,70`, `commands_chat.rs:70`, `engine.rs:268`
- **What's wrong:** `fs::write` errors return early unreported, `rename(..).ok()` swallows the result. Four commands report success to the UI when the config never reached disk — the user's leagues disappear on next launch with no error.
- **Fix:** Return `Result<(), String>` and propagate at all four call sites (`write_cache_checked` at `engine.rs:177` is the in-repo precedent).
- **Effort:** S
- **Grade lift:** B → B+ (truthfulness about persistence)

#### ~~B5~~ ✓ done 2026-08-30 — The `season` mutex is held across up to ~25s of network retries
- **Where:** `src-tauri/src/commands_season.rs:77-79,122-125`; `refresh_live` at `season_engine.rs:389-399`
- **What's wrong:** Three requests × 8s timeout × 3 retries can hold the `season` lock for tens of seconds, blocking `get_season`, `load_season`, and `ask_claude`, and queuing the next poll tick behind the current one.
- **Fix:** Fetch outside the lock (`tokio::join!` the three requests unlocked), then take the lock only to run `apply_refresh`.
- **Effort:** S
- **Grade lift:** B → B+ (keeps the UI responsive during network trouble)

#### ~~B6~~ ✓ done 2026-08-30 — Keychain subprocess runs blocking on the runtime thread, once per chat request
- **Where:** `src-tauri/src/secrets.rs:56-67`; reached from `commands_chat.rs:53,83,149` via `engine.rs:246-251`
- **What's wrong:** `std::process::Command` + `wait_with_output` on the async thread — tens of milliseconds normally, unbounded if the Keychain prompts — with `config` locked, re-shelled on every `ask_claude` and `chat_settings`.
- **Fix:** Switch to `tokio::process::Command` (already a dependency, used in `chat_cli.rs:15`) or wrap in `spawn_blocking`; cache the loaded key in `AppState`.
- **Effort:** S
- **Grade lift:** B → B (removes a hidden stall and a per-request subprocess)

#### ~~B7~~ ✓ done 2026-08-30 — Weekly rollover refetches all ~15 weeks; transactions fetched sequentially with O(n²) dedupe
- **Where:** `src-tauri/src/season_engine.rs:118-170` (`week_sweep`), `:340-355` (transactions)
- **What's wrong:** Completed weeks' matchups are immutable, but `sweep.week != week` invalidates the whole sweep — 15 requests to learn one new week. The transaction loop is sequential where every sibling path uses `join!`, and dedupes with `.iter().any()` over a growing Vec.
- **Fix:** Cache per-week (`season_<id>_week<N>.json`), fetch only missing weeks plus the current one; `join!` the two transaction weeks and dedupe with a `HashSet` of ids.
- **Effort:** S
- **Grade lift:** B → B (faster weekly rollover, less Sleeper load)

---

## C — Frontend Quality — B

Component discipline, strict TypeScript, and the store pattern remain
strengths, and there are no hardcoded colors outside `theme.css` (verified: 0
hits). But the deep pass found two real state bugs in the season lifecycle and
a systematic accessibility gap: ARIA is applied but incomplete (no focus trap
on the confirm dialog, no live announcement of the app's most important
moment, load-bearing info in hover-only tooltips), and nothing lints for it.

#### ~~C1~~ ✓ done 2026-08-30 — The season poll is torn down and restarted on every incoming update
- **Where:** `src/session.ts:47-67`
- **What's wrong:** `season` is in the effect's dependency array and the cleanup calls `api.stopSeasonPolling()`. Every push from `onSeasonUpdated` changes `season`, so the backend's 30s timer is cancelled and recreated on each tick — an unbounded stop/start race in which the interval never runs its own schedule.
- **Fix:** Split into two effects: one keyed on `[active, ready]` that owns start/stop polling; one for the initial load guarded by a `useRef` instead of reading `season` from deps.
- **Effort:** S
- **Grade lift:** B → B+ (fixes a live-polling correctness bug)

#### ~~C2~~ ✓ done 2026-08-30 — The Live badge freezes instead of degrading when the stream dies
- **Where:** `src/components/SeasonScreen.tsx:64-88`; swallowed rejection at `session.ts:61,65`
- **What's wrong:** The badge computes its age during render, and the screen only re-renders when data arrives — so if the poll dies, the badge reads "Live · 8s ago" forever. When `sources` is absent it renders `pill-live` with no staleness check. `startSeasonPolling` rejections are `.catch(() => undefined)`-ed, so a poller that never starts produces a frozen screen with zero signal.
- **Fix:** A 10s heartbeat interval inside `LiveBadge` so age recomputes without data; apply the staleness threshold in the `sources === undefined` branch too; route the rejections through `onError`.
- **Effort:** S
- **Grade lift:** B → B+ (the status feature becomes trustworthy under failure — its whole point)

#### ~~C3~~ ✓ done 2026-08-30 — "You are on the clock" is announced to nobody
- **Where:** `src/components/ClockBanner.tsx:44`; only two `role="status"` and one `aria-live` exist in the tree
- **What's wrong:** The app's most important state change renders as a plain styled span. A screen-reader user gets a chime (if enabled) and no text; the `is-mine` highlight is color-only.
- **Fix:** Wrap the clock-main block in `role="status" aria-live="assertive" aria-atomic="true"` rendering a stable sentence ("You are on the clock — pick 3.07, 41 seconds left"), announced once per transition rather than every second.
- **Effort:** S
- **Grade lift:** B → B+ (the headline moment becomes accessible)

#### ~~C4~~ ✓ done 2026-08-30 — `ConfirmDialog` claims `aria-modal` but doesn't trap focus
- **Where:** `src/components/Overlays.tsx:17-50`; the working trap lives in `bits.tsx:79-105` (`ZoomLayer`)
- **What's wrong:** Tab walks straight out of the dialog into the 200-row board behind the scrim; the pattern exists in the repo and this dialog was left out of it.
- **Fix:** Extract `ZoomLayer`'s keydown trap into a `useFocusTrap(ref)` hook in `bits.tsx`; use it in both; set `inert`/`aria-hidden` on `.shell` while an overlay is open.
- **Effort:** S
- **Grade lift:** B → B (consistency with the app's own best practice)

#### C5 — Failed actions vanish in a 5-second toast with no retry
- **Where:** `src/App.tsx:216-261` (`doDraft`, `doUndo`, `doExport`, `doRefreshData`, `togglePolling`); auto-dismiss at `:94`
- **What's wrong:** Every mutating rejection becomes `showToast(String(e))` — raw Rust error text that auto-dismisses in 5s with no retry affordance. A failed "Mark drafted" during a live draft disappears before the user can react.
- **Fix:** Extend `Toast` with an optional `{ label, onClick }` action; pass a retry that re-runs the call; suppress auto-dismiss whenever an action is present.
- **Effort:** S
- **Grade lift:** B → B+ (draft-night failures become recoverable)

#### ~~C6~~ ✓ done 2026-08-30 — The settings menu has no menu semantics, state exposure, or focus management
- **Where:** `src/components/Header.tsx:145,222-260`
- **What's wrong:** No `aria-haspopup`/`role="menu"`; toggle state lives only in a styled span (no `aria-checked`); focus never moves in or restores; dismissal is mousedown-only so tabbing away leaves it open.
- **Fix:** `role="menu"` on the container, `role="menuitemcheckbox" aria-checked` per row, focus first row on open / restore the gear on close, add a `focusout` handler.
- **Effort:** S
- **Grade lift:** B → B (completes the menu's half-built ARIA)

#### ~~C7~~ ✓ done 2026-08-30 — Load-bearing information exists only in mouse-hover `title=`
- **Where:** `src/components/bits.tsx:266` (injury tag), `Header.tsx:30,38` (sync error detail), `SeasonScreen.tsx:79,87` (per-source freshness)
- **What's wrong:** The injury word ("Questionable") and the *only* diagnostics for why sync is failing live in `title` on non-focusable spans — unreachable by keyboard, unreliable for screen readers.
- **Fix:** `aria-label` + visually-hidden text on the injury tag; make the sync pill a button that expands an inline detail line (or render source lines under the badge whenever a source is behind).
- **Effort:** S
- **Grade lift:** B → B+ (the new status work becomes available to everyone)

---

## D — Testing & Reliability — B

The corpus is genuinely good — 212 Rust tests (74.77% lines), 160 frontend
tests (88.97% lines), property-style coverage of the solvers, golden tests on
the view. But the gates are weaker than they look: the frontend coverage
thresholds are dead configuration that nothing executes, the most stateful
backend module is at 0%, no test anywhere drives the real Tauri app, and
today's lazy-chunk split added a failure mode (rejected `import()`) with no
error boundary to catch it.

#### ~~D1~~ ✓ done 2026-08-30 — [FE] The frontend coverage gate is dead configuration
- **Where:** `vitest.config.ts:12` (thresholds declared) vs `package.json:12` (`vitest run`, no `--coverage`) and `.github/workflows/verify.yml:41`
- **What's wrong:** The 80/80/75/70 thresholds only fire if a human types `--coverage` by hand. Rust is gated in CI; the frontend is not. A commit dropping `App.tsx` to 40% passes green.
- **Fix:** Add `"test:coverage": "vitest run --coverage"`; run it in CI's verify step; keep bare `vitest run` for the fast local loop.
- **Effort:** S
- **Grade lift:** B → B+ (turns claimed gates into real ones)

#### ~~D2~~ ✓ done 2026-08-30 — [both] The season poller has no health channel; failures are silently eaten
- **Where:** `src-tauri/src/commands_season.rs:124` (`.is_ok()` discards the error); `src/session.ts:61,65` (`.catch(() => undefined)`)
- **What's wrong:** The draft poller aggregates errors and emits `poll-health`; the season poller emits nothing on failure. Sleeper can be down all Sunday and the UI shows a stale scoreboard with no signal. (Pairs with C2 — this is the backend half.)
- **Fix:** Collect the error, call `poll::record_poll_outcome`, emit a `season-poll-health` event, render it in `SeasonScreen`; replace both swallowed rejections with `setError`.
- **Effort:** M
- **Grade lift:** B → B+ (reliability signal for the season half of the app)

#### ~~D3~~ ✓ done 2026-08-30 — [BE] `commands_season.rs` is 0.00% covered — the most stateful backend file
- **Where:** `src-tauri/src/commands_season.rs` (190/190 regions missed); no integration test references it
- **What's wrong:** The 30s poller loop, `LiveEmitGate`/`AnalysisCache` wiring, and the generation guard live entirely outside test reach because the loop body is welded to `tauri::async_runtime::spawn`.
- **Fix:** Extract the loop body into `async fn season_tick(...) -> TickOutcome` in `poll.rs` (the `DraftPollMemory` precedent) and unit-test it against `mock_league.rs`. Combines naturally with D2.
- **Effort:** M
- **Grade lift:** B → B+ (the draft poller got this treatment; the season poller never did)

#### ~~D4~~ ✓ done 2026-08-30 — [FE] No error boundary — a failed lazy chunk blanks the whole window
- **Where:** `src/App.tsx:395,399,415` (Suspense wrappers); zero `ErrorBoundary`/`componentDidCatch` hits in the tree
- **What's wrong:** Suspense handles the pending promise, not a rejected one. A corrupt chunk or torn install unmounts the entire tree to a blank window — a failure mode today's split made non-hypothetical.
- **Fix:** A ~20-line `ErrorBoundary` class around each Suspense with a "Reload" button; test by throwing from a mocked lazy import.
- **Effort:** S
- **Grade lift:** B → B+ (contains the new failure mode)

#### D5 — [BE] Two untested pieces from today: `season_lookup.rs` (48%) and `AnalysisCache`
- **Where:** `src-tauri/src/season_lookup.rs:18,29,47,59` (no test block; the missing-metadata fallbacks are exactly what's uncovered); `poll.rs:107-140` (`AnalysisCache` — the mod tests below it cover its two neighbors but not it)
- **What's wrong:** Every season section resolves names through `Lookup`'s fallback layer, untested; `AnalysisCache`'s `ticks % rebuild_every` expiry and `.max(1)` clamp are off-by-one-prone and unverified.
- **Fix:** Tests for absent id / missing position / missing team / injury None-vs-Some; three asserts on the cache (None before first observe, held through tick 19, None at tick 20).
- **Effort:** S
- **Grade lift:** B → B (closes today's coverage stragglers)

#### D6 — [FE] The worst frontend files sit below the thresholds the config claims
- **Where:** `src/prefs.ts` 57.9% stmts / 0% branches; `src/components/Board.tsx` 68.5% stmts; `src/App.tsx` 68.8% stmts with lines 392-423 (the whole screen-switch/Suspense/season-error branch) dark; `src/theme.ts` 53.3% branches; `src/session.ts` 64.3% functions
- **What's wrong:** The primary screen and the persistence layer are the least-covered files in the repo — invisible until D1 lands, at which point they fail the gate.
- **Fix:** `prefs.ts`/`theme.ts` are pure — test first; then a `Board.tsx` pass for the empty/filtered branches and an `App.tsx` test through the screen-switch path.
- **Effort:** M
- **Grade lift:** B → B+ (do before or with D1 so the gate lands green)

#### D7 — [both] Nothing ever launches the real app — confirmed no e2e harness exists
- **Where:** no `tauri-driver`/Playwright/WebdriverIO anywhere (checked); `lib.rs` also 0% covered
- **What's wrong:** Command registration, the capability set, the CSP, and the bundle are never exercised together in a real WKWebView — the most likely place for a shipping break.
- **Fix:** One smoke spec via `tauri-driver` + WebdriverIO: launch, load the committed fixture league, assert the board renders. A single test closes the "does it boot" gap.
- **Effort:** L
- **Grade lift:** B → B+ (the one class of regression no current test can catch)

#### D8 — [FE] Time-dependent tests run against the real clock
- **Where:** `src/format.test.ts:52,59` (`Date.now() + ms + 30_000` fudge), `SeasonTabs.test.tsx:99`, `SeasonScreen.test.tsx:6`
- **What's wrong:** The 30s fudge makes tests less likely to fail rather than deterministic, and the interesting rollover boundary is never actually asserted. `ClockBanner.test.tsx:16` already shows the right pattern.
- **Fix:** `vi.useFakeTimers()` + `vi.setSystemTime(...)` in the four files; assert the exact boundaries the fudge hides.
- **Effort:** S
- **Grade lift:** B → B (removes latent flakiness)

---

## E — Security — A−

Genuinely strong for a local-first app: restrictive CSP verified in the
running webview, key off argv with a test guarding it, no secrets in the repo,
clean audits, host-constant fetching, validated inputs. What remains is
defense-in-depth: CI trust, file permissions, and one unused capability.

#### ~~E1~~ ✓ done 2026-08-30 — CI actions on floating tags, no `permissions:` block, no job timeouts
- **Where:** `.github/workflows/verify.yml:20,24,36,64,68,75-77`
- **What's wrong:** Six actions pinned to mutable tags — including third-party `rustsec/audit-check@v2`, which is handed `secrets.GITHUB_TOKEN`. No top-level `permissions:` (token gets repo default, not `contents: read`); no `timeout-minutes`, so a hung build burns up to 6 hours at 10× macOS billing.
- **Fix:** Pin all six to commit SHAs with version comments; add `permissions: { contents: read }`; `timeout-minutes: 30`/`10` on the two jobs.
- **Effort:** S
- **Grade lift:** A− → A (closes the supply-chain gap in CI)

#### ~~E2~~ ✓ done 2026-08-30 — League cache written world-readable; only `config.json` gets 0600
- **Where:** `src-tauri/src/engine.rs:106` (dir at 0755), `cache.rs:51` (files at 0644); the correct pattern exists at `engine.rs:233-237`
- **What's wrong:** Rosters, users, league member names/ids, and the players dictionary are readable by any local process; the repo demonstrably knows the fix and applies it to one file only.
- **Fix:** `set_permissions(0o700)` on the data dir in `Engine::new`; add the existing `#[cfg(unix)]` 0600 block to `cache::replace_file` (and `headshots.rs:124`'s dir).
- **Effort:** S
- **Grade lift:** A− → A− (consistency; low sensitivity but zero cost)

#### ~~E3~~ ✓ done 2026-08-30 — `tauri-plugin-opener` is registered and granted a capability nothing uses
- **Where:** `src-tauri/src/lib.rs:67`, `capabilities/default.json` (`opener:default`); zero frontend usages (checked)
- **What's wrong:** The webview holds a live open-url/open-path permission no code path needs — a free XSS-to-shell escalation step in a webview whose CSP permits inline styles.
- **Fix:** Drop the plugin init, the capability entry, and both dependency entries; re-add scoped with an allowlist if a link ever needs opening.
- **Effort:** S
- **Grade lift:** A− → A (attack-surface reduction for free)

---

## F — Dependencies & Tech Currency — A−

Lean and current, both lockfiles committed, audits clean, toolchain pinned and
read by CI. Two pinning inconsistencies remain.

#### ~~F1~~ ✓ done 2026-08-30 — npm side ignores the pinning rule the Cargo side documents
- **Where:** `package.json:31-33` (`@tauri-apps/api: "^2"`, `@tauri-apps/cli: "^2"`) vs the pinning rationale at `src-tauri/Cargo.toml:19-31`; `.nvmrc`=22 but no `.npmrc`
- **What's wrong:** A clean `npm install` can pull Tauri JS 2.99 against Rust pinned at 2.11 — exactly the IPC-ABI drift the Cargo comment exists to prevent. And `engines` isn't enforced without `engine-strict`, so Node 20 fails confusingly instead of clearly.
- **Fix:** Pin both to `^2.11`; add `.npmrc` with `engine-strict=true`.
- **Effort:** S
- **Grade lift:** A− → A (one pinning philosophy, both ecosystems)

#### ~~F2~~ ✓ done 2026-08-30 — Mobile-only crate types slow every build of a macOS-only app
- **Where:** `src-tauri/Cargo.toml:12` (`crate-type = ["staticlib", "cdylib", "rlib"]`)
- **What's wrong:** `staticlib`/`cdylib` exist only for iOS/Android — targets the same file's reqwest comment says aren't real — and add two link steps to every build.
- **Fix:** Reduce to `["rlib"]` (or cfg-gate the mobile types).
- **Effort:** S
- **Grade lift:** A− → A− (faster builds, honest manifest)

---

## G — Performance & Scalability — B

The headline fixes are real (odds sim 39→14ms, board identity caching, the
analysis cache). But the second tier is substantial: the analysis cache is
incomplete so three sections still rebuild per tick, the board emit deep-clones
~700 players every 3 seconds, the 14.6 MB parse still lands on the runtime
thread via the network path, and the frontend re-renders the draft screen twice
a second from duplicate clocks.

#### ~~G1~~ ✓ done 2026-08-30 — Three sections rebuild every 30s from inputs that only change at load
- **Where:** `src-tauri/src/season.rs:277-281`; `refresh_live` writes only matchups/scores/rosters (`season_engine.rs:389-399`)
- **What's wrong:** `activity`, `recent_trades`, and `trends` (a 40-snapshot × 12-team diff) depend solely on `transactions`/`history`, set once at load — yet run on every tick. This is the exact waste `SeasonAnalysis` exists to eliminate; the fields were never added.
- **Fix:** Add all three to `SeasonAnalysis` and `::of`; read from `cached` in the same `match` pattern as lines 173/199/207.
- **Effort:** S
- **Grade lift:** B → B+ (finishes the analysis cache's own design)

#### ~~G2~~ ✓ done 2026-08-30 — All ten stylesheets load eagerly, blunting today's code-split
- **Where:** `src/App.tsx:22-31`; ~1,400 lines (~60% of CSS) belong to lazy screens
- **What's wrong:** JS is split but `board.css`, `season*.css`, `trends.css`, `live.css`, `chat.css` still ship in the entry chunk and parse before first paint.
- **Fix:** Move each sheet's import into its owning lazy component; Vite emits per-chunk CSS automatically. Keep `theme/App/components/zoom` eager.
- **Effort:** S
- **Grade lift:** B → B+ (completes the lazy-load story)

#### ~~G3~~ ✓ done 2026-08-30 — The board emit deep-clones ~700 players and rescans per position every 3s
- **Where:** `src-tauri/src/view.rs:271-280` (clone), `:282-304` (per-position tier scan ≈ 3,600 iterations), `:308-310` (per-pick String building to inspect 6 picks) — all under lock in the poll tick
- **What's wrong:** Four owned strings per player per tick, then a full-vector walk per draftable position, then JSON serialisation of it all.
- **Fix:** One-pass tier map (`HashMap<&str,(u32,u32)>`); slice the last 6 picks; serialise a borrowed view struct (or emit top-N with a tail command).
- **Effort:** M
- **Grade lift:** B → B+ (the draft tick's biggest remaining cost)

#### ~~G4~~ ✓ done 2026-08-30 — The 14.6 MB players parse still blocks the runtime — via the network path
- **Where:** `src-tauri/src/sleeper.rs:285-287` (`resp.json::<T>()` deserialises in-task); only the disk read was moved off-thread
- **What's wrong:** Cold loads block the executor for hundreds of milliseconds on the exact parse the doc comment at `engine.rs:132-137` warns about.
- **Fix:** `resp.bytes().await` then `spawn_blocking(|| serde_json::from_slice(...))` in `projections.rs::players` (add a `get_bytes` helper; don't change the shared path).
- **Effort:** S
- **Grade lift:** B → B (finishes B3-from-the-first-audit properly)

#### ~~G5~~ ✓ done 2026-08-30 — League load serialises six independent fetches
- **Where:** `src-tauri/src/engine.rs:271-292` (league → draft → users sequential), `:354-359` (players, season projections, weekly projections sequential — the third an 18-request fan-out)
- **What's wrong:** At 8s timeout each, a cold load pays serial latency for parallel work; sibling files already use `join!`/`buffer_unordered`.
- **Fix:** `tokio::join!` draft with users; `try_join!` the three projection loads.
- **Effort:** S
- **Grade lift:** B → B+ (~one round of latency instead of several on first load)

#### ~~G6~~ ✓ done 2026-08-30 — The waiver/standings inner loops re-clone what a scratch buffer already solved
- **Where:** `src-tauri/src/season_moves.rs:62-70` (`base.to_vec()` per candidate ≈ 23k String allocs/rebuild); `season_view_standings.rs:39` + `season_lineup.rs:100-115` + `season_lookup.rs:17-24` (candidates rebuilt per week ≈ 10.8k allocs)
- **What's wrong:** `season_trades.rs:47-59` established the `scratch: &mut Vec` pattern; these two call sites weren't converted. Positions are week-invariant but resolved per week.
- **Fix:** Give `marginal_gain` the scratch parameter; hoist candidate construction per roster and overwrite only `points` per week.
- **Effort:** S
- **Grade lift:** B → B (applies the repo's own established fix)

#### G7 — Draft screen re-renders twice a second from duplicate clocks; several unmemoized per-render scans
- **Where:** `src/components/ClockBanner.tsx:15-23,29,115` (two independent 1s intervals + unmemoized `buildQueue`); `Panels.tsx:195-199` (`atRisk` filter/sort per render); `DraftScreen.tsx:22-26` (O(n²) dedupe + three full scans); `TrendsTab.tsx:106-224` (14 SVG paths rebuilt per mousemove; `Math.min(...spread)` will eventually throw); `Board.tsx:297` ("Show all" renders ~600 rows / ~1,800 nodes in one commit, and the loading state at `:267` lacks `role="status"`)
- **What's wrong:** Each is small; together they undo the render work G6-from-the-first-audit paid for.
- **Fix:** One clock in a `useSyncExternalStore` module store; `useMemo` for `buildQueue`/`atRisk`/rank-Map/chart paths (reduce instead of spread); incremental paging for Show all; `role="status"` on the loader.
- **Effort:** M
- **Grade lift:** B → B+ (a bundle of S-fixes; do as one pass)

#### ~~G8~~ ✓ done 2026-08-30 — Remaining blocking file I/O on the runtime thread
- **Where:** `src-tauri/src/headshots.rs:123-168` (all fs ops in `cached_image`, called dozens of times concurrently per roster render); `season_history.rs:145-157` (multi-MB history read-modify-write per `load_season`)
- **What's wrong:** The `*_off_thread` helpers exist for exactly this and these two paths don't use them.
- **Fix:** Route both through `read_cache_any_off_thread`/`write_cache_off_thread` (or `tokio::fs` for the images).
- **Effort:** S
- **Grade lift:** B → B (closes the last off-thread stragglers)

---

## H — Documentation & Onboarding — B−

The READMEs are well-written where they're current, and 41+ Rust files carry
module headers. But today's velocity outran the docs: the module map is
missing 9 of 49 Rust files (including three added today), the frontend map is
one line, the poller/caching architecture has no prose anywhere, and the one
documented fixture-regeneration step names a path that doesn't exist on macOS.

#### ~~H1~~ ✓ done 2026-08-30 — The fixture-regeneration instructions point at the wrong path
- **Where:** `draft-assistant/README.md` (Browser preview: "delete `/tmp/draft-assistant-cli`"); actual path is `std::env::temp_dir()` → `$TMPDIR/draft-assistant-cli` (`bin/dump_season.rs:38`, `dump_state.rs:52`)
- **What's wrong:** Following the doc deletes nothing, so you regenerate fixtures from stale cache — precisely the failure the surrounding paragraph warns about. It's the only documented dev loop for the browser preview.
- **Fix:** `rm -rf "${TMPDIR:-/tmp}/draft-assistant-cli"`, or better, add a `--fresh` flag to both binaries.
- **Effort:** S
- **Grade lift:** B− → B (the one actively wrong instruction)

#### ~~H2~~ ✓ done 2026-08-30 — The poll/caching architecture exists only inside the code that implements it
- **Where:** no README-level prose for: two pollers (3s draft, 30s season), `LiveEmitGate`, `AnalysisCache`/`ANALYSIS_EVERY`, `season_generation`; "Data sources" documents disk TTLs only
- **What's wrong:** "Why didn't the screen update" is the most likely debugging question, and its answer is undocumented.
- **Fix:** A "How it stays live" README section — one table: two pollers, three caches, what invalidates each.
- **Effort:** S
- **Grade lift:** B− → B

#### ~~H3~~ ✓ done 2026-08-30 — The Rust module map is missing 9 of 49 files, including three from today
- **Where:** `draft-assistant/README.md:130-180`; missing `season_calls.rs` (the biggest new module), `season_injury.rs`, `season_sources.rs`, `season_types.rs`, `season_trends_view.rs`, `cache.rs`, `poll.rs`, `mock_league.rs`, `main.rs`
- **What's wrong:** The map was updated for one of today's two refactors and not the other.
- **Fix:** Add the nine lines under the existing groupings.
- **Effort:** S
- **Grade lift:** B− → B

#### ~~H4~~ ✓ done 2026-08-30 — The frontend layout map is a single line
- **Where:** `README.md:132-136` — five entries for all of `src/`; `session.ts`, `boardIdentity.ts`, `lazyScreens.tsx`, `prefs.ts`, `theme.ts`, `format.ts`, both types files are absent
- **What's wrong:** A newcomer can't discover that `session.ts` owns the polling lifecycle or that `boardIdentity.ts` is a subtle load-bearing cache.
- **Fix:** Expand `src/` to the per-file granularity the Rust side gets.
- **Effort:** S
- **Grade lift:** B− → B

---

## I — Developer Experience & Tooling — B+

The verify chain, hooks, pinned toolchains, and CI caching are all real. The
gaps: the lint config isn't type-aware (so the promise bugs in C1/C2 were
invisible to it), nothing lints accessibility, the pre-commit hook runs zero
tests, and the Rust coverage floor has 6.8 points of dead slack.

#### ~~I1~~ ✓ done 2026-08-30 — ESLint isn't type-aware; the season-poll bugs were invisible to it
- **Where:** `eslint.config.js:22-26` (`recommended`, not `recommendedTypeChecked`)
- **What's wrong:** No `no-floating-promises`/`no-misused-promises` in an app whose whole data layer is async IPC; it would have flagged `session.ts:61,65` directly.
- **Fix:** Switch to `recommendedTypeChecked` with `projectService: true`; fix the handful of real findings the first run surfaces.
- **Effort:** M
- **Grade lift:** B+ → A− (the highest-value rule set available to this codebase)

#### I2 — Nothing lints accessibility
- **Where:** `eslint.config.js:20-36` — no `eslint-plugin-jsx-a11y`
- **What's wrong:** Every issue in C3/C4/C6/C7 is in the class that plugin catches automatically; `--max-warnings=0` means it becomes a gate the moment it's added.
- **Fix:** Add `jsx-a11y` (strict preset) to the `src/**` block; fix what it finds (overlaps the C items).
- **Effort:** M
- **Grade lift:** B+ → A− (stops the a11y gap from regrowing)

#### ~~I3~~ ✓ done 2026-08-30 — The pre-commit hook runs zero tests and coverage never runs locally
- **Where:** `.githooks/pre-commit` (five lint/type steps); `package.json:23` (`verify:fast`, same set); `test:frontend` has no `--coverage`
- **What's wrong:** The fastest behavioral signal is a full `verify` (with a vite build and full cargo test) or a CI round trip — no middle rung; no developer ever sees a coverage number locally.
- **Fix:** Add `verify:mid` = `verify:fast` + `test:frontend` (~10s measured) and make it the hook; keep cargo tests in full verify.
- **Effort:** S
- **Grade lift:** B+ → B+ (a 10-second behavioral gate before every commit)

#### ~~I4~~ ✓ done 2026-08-30 — The Rust coverage floor can't catch a regression, and the tool install swallows failures
- **Where:** `.github/workflows/verify.yml:59-60` — `--fail-under-lines 68` vs measured 74.77%; `cargo install cargo-llvm-cov --locked || true`
- **What's wrong:** 6.8 points of slack — enough to delete every test in `season_calls.rs` and pass; the `|| true` turns install failures into a confusing "command not found" a line later.
- **Fix:** Raise to `--fail-under-lines 74`; pin the cargo-llvm-cov version; drop `|| true`.
- **Effort:** S
- **Grade lift:** B+ → B+ (the gate becomes taut)
