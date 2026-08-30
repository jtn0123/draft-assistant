# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-30 (rerun; supersedes the 2026-08-27 report, which predates the ~17k-line season-mode commit `25b1c38`)
**Stack:** Tauri 2 desktop app — Rust/Tokio backend + React 19 / TypeScript-strict / Vite frontend, ~18.4k LOC first-party

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B− | 6 |
| B | Backend Quality | B− | 7 |
| C | Frontend Quality | B | 7 |
| D | Testing & Reliability | B− | 6 |
| E | Security | A− | 5 |
| F | Dependencies & Tech Currency | A− | 4 |
| G | Performance & Scalability | C+ | 8 |
| H | Documentation & Onboarding | C+ | 5 |
| I | Developer Experience & Tooling | B− | 6 |
| **Overall** | | **B−** | **54** |

**Top 5 highest-leverage fixes:** B1, D1, B2, G5, E1

**Vs. the 2026-08-27 audit (overall C+):** version control now exists and is CI-gated, the deadlock finding no longer reproduces in the current module layout, and Security/Deps improved materially. New season-mode analysis code introduced most of the Performance findings.

**Measured coverage at audit time:** Rust 57.3% lines (cargo-llvm-cov; 104 tests + 2 integration), frontend 71.8% lines / 69.8% statements (vitest v8; 81 tests). Excluding entry-point glue (`main.rs`, `lib.rs`, `bin/`), Rust is materially higher; per-file detail under D.

---

## A — Architecture & Design — B−

Layering intent is real and documented: `sleeper.rs`/`season_api.rs` are pure HTTP clients, `engine.rs` owns caching + assembly, `view.rs`/`season.rs` are pure view assembly, `commands_*.rs` sit on top; dependency direction is one-way with no cycles, and the 500-LOC cap keeps every file readable. Dragged down by: `Engine` as a de-facto god object extended from five files, the season layer reaching back down into the transport layer, non-thin commands, and no frontend state layer.

#### A1 — `Engine` is extended from 5 files, so its real surface is invisible
- **Where:** `src-tauri/src/engine.rs:96`, `projections.rs:12`, `headshots.rs:101`, `season_engine.rs:75`, `season_history.rs:130`
- **What's wrong:** Five `impl Engine` blocks bolt unrelated concerns (image CDN, league sweep) onto one struct; all share private access to `read_cache`/`write_cache`. Nothing states what `Engine` is.
- **Fix:** Keep `Engine` as thin `{ client, data_dir, cache }`; express the rest as traits (`ImageCache`, `SeasonLoader`) or free functions taking `&Cache` so seams are declared and mockable.
- **Effort:** M
- **Grade lift:** B− → B (the largest structural ambiguity in the backend)

#### A2 — Inverted dependency: the season layer extends the transport layer
- **Where:** `src-tauri/src/season_api.rs:255` (and duplicated `BASE`/`BASE_UNDOC` at `season_api.rs:11-12` vs `sleeper.rs:11-12`)
- **What's wrong:** A season-screen module opens `impl SleeperClient` and owns HTTP methods, splitting the client's route list across two files with duplicated base URLs.
- **Fix:** Move the six methods (`nfl_state`, `rosters`, `matchups`, `transactions`, `winners_bracket`, `nfl_scores`) into `sleeper.rs`, or define an explicit `SeasonEndpoints` extension trait.
- **Effort:** S
- **Grade lift:** B− → B− (clarity; removes duplicate constants)

#### A3 — Commands are not thin: 100-line polling loops inline
- **Where:** `src-tauri/src/commands_draft.rs:207-305` (`start_polling`), `commands_season.rs:82-142` (`start_season_polling`)
- **What's wrong:** Each embeds a full spawn-loop with change detection, error aggregation, reconciliation and emit logic; the tick logic has zero tests because it's welded to Tauri state.
- **Fix:** Extract `async fn poll_once(&engine, &state) -> Option<PollOutcome>` into a `poll.rs`; command becomes spawn + generation bookkeeping; unit-test the tick.
- **Effort:** M
- **Grade lift:** B− → B (also unlocks D coverage of the poll path)

#### A4 — The Sleeper client is bypassed inside a command
- **Where:** `src-tauri/src/commands_draft.rs:60-62` (`set_my_username`)
- **What's wrong:** Bare `reqwest::get` with an inline `struct User` skips the pooled client, its 3s/8s timeouts, gzip, and user-agent.
- **Fix:** Add `SleeperClient::user(&self, username)` next to `league()`/`draft()`; call through `state.engine.client`.
- **Effort:** S
- **Grade lift:** B− → B− (consistency; also see E4)

#### A5 — `season.rs` is the season god-module; `season_view_parts.rs` is an LOC-cap artifact
- **Where:** `src-tauri/src/season.rs:4-18` (15 crate imports), `season_view_parts.rs:56-99`
- **What's wrong:** `build_season_view` couples to 15 modules (2× anything else); `season_view_parts.rs`'s only theme is "helpers that didn't fit."
- **Fix:** Split `build_season_view` by section (matchup / waivers / standings) so each sub-builder imports 2-3 modules; rehome `matchup_for`/`opponent_of` → `season_api.rs`, `why_start` → `season_lineup.rs`.
- **Effort:** M
- **Grade lift:** B− → B

#### A6 — No frontend state layer; `App.tsx` is the store
- **Where:** `src/App.tsx:39-67` (~16-20 `useState`), `:73-206` (9 `useEffect`s)
- **What's wrong:** Every panel gets its slice via prop drilling through `DraftScreen`/`SeasonScreen`; the polling/refresh state machine is untestable in isolation.
- **Fix:** Extract `useDraftSession()` / `useSeasonSession()` hooks (view + polling + error + reload token) into `src/session.ts`, matching the existing `avatars.ts`/`zoom.ts` store pattern.
- **Effort:** M
- **Grade lift:** B− → B

---

## B — Backend Quality — B−

Production error handling is disciplined: essentially every `unwrap`/`expect` is inside `#[cfg(test)]`, serde structs degrade gracefully via `#[serde(default)]`/`Option`, cache filenames are sanitized (`engine.rs:152-158`), writes are tmp-then-rename atomic, and `headshots.rs:32-47` refuses any non-Sleeper URL. Against that: one live division-by-zero panic path fed by remote data, a refresh that reports success when every request failed, all disk I/O blocking inside async fns (including a ~14.6 MB JSON parse), and zero retries anywhere.

#### B1 — Panic on remote data: division by zero in the hot render path
- **Where:** `src-tauri/src/draft.rs:10-11`; fed from `sleeper.rs:59` (`DraftSettings.teams`), called at `view.rs:187`
- **What's wrong:** `(pick_no - 1) / teams` with no zero guard; `teams: 0` panics the command task on every view build. `(pick_no - 1)` also underflows with `overflow-checks = true` in release.
- **Fix:** Validate once in `Engine::assemble` (`engine.rs:295`): `teams == 0 || rounds == 0` → `Err("draft has no teams/rounds")`; make `slot_for_pick` return `Option<u32>`.
- **Effort:** S
- **Grade lift:** B− → B (removes the only known remote-data panic)

#### B2 — `refresh_live` cannot fail, and lies about freshness
- **Where:** `src-tauri/src/season_engine.rs:325-346`; downstream `:353-356` (`live_is_stale`), `commands_season.rs:115`
- **What's wrong:** All three fetch results consumed with `if let Ok(..)`, then `fetched_at = now_secs(); Ok(())` unconditionally — a total outage still resets the staleness clock and the health badge stays green.
- **Fix:** Collect errors; stamp `fetched_at` only if ≥1 succeeded; return `Err(errors.join("; "))` when all fail.
- **Effort:** S
- **Grade lift:** B− → B (truthful health reporting during games)

#### B3 — Blocking file I/O on the async runtime, including a ~14.6 MB parse
- **Where:** `src-tauri/src/engine.rs:125-140`, called from async `projections.rs:18,22,25`; also `headshots.rs:127,132,162,169`, `commands_draft.rs:201`
- **What's wrong:** `std::fs` + `serde_json::from_str` of the full `players/nfl` dictionary on the runtime thread; no `spawn_blocking` anywhere in the tree.
- **Fix:** Wrap `read_cache`/`write_cache` bodies in `tokio::task::spawn_blocking` (cheapest; keeps sync helpers).
- **Effort:** S
- **Grade lift:** B− → B−

#### B4 — No retries/backoff; long sequential request chains
- **Where:** `src-tauri/src/sleeper.rs:258-276` (`get_json`), `projections.rs:91-105` (18 sequential weekly requests), `season_engine.rs:105-120` (~15 sequential matchup requests)
- **What's wrong:** First transient error is permanent; a flaky network makes `load_league` hang for minutes at the 8s timeout per call.
- **Fix:** Add `get_json_retry` (2 retries, 250ms→1s jitter, transport errors + 5xx only); replace both loops with `buffer_unordered(6)`.
- **Effort:** M
- **Grade lift:** B− → B (load time ~5× better on the sweep, resilient to blips)

#### B5 — Tauri commands accept unbounded/unvalidated input
- **Where:** `src-tauri/src/commands_chat.rs:114-121` (`ask_claude`), `commands_draft.rs:139` (`record_manual_pick`), `commands_draft.rs:11-18` (`extract_id`)
- **What's wrong:** Unbounded `messages` forwarded to Anthropic/CLI; `record_manual_pick` persists ids that don't exist on the board; `extract_id` falls through to raw input (see E2).
- **Fix:** Cap messages (~40 turns / 200 KB); `record_manual_pick` → `Err("unknown player")` unless in `board_index`; `extract_id` rejects non-`[0-9]{8,20}`.
- **Effort:** S
- **Grade lift:** B− → B−

#### B6 — API key passed on the process command line
- **Where:** `src-tauri/src/secrets.rs:23` (`["-w", key]`), `:36-39`; `panic!` in a pub fn at `:19`
- **What's wrong:** The raw `sk-ant-…` key is visible in `ps aux` during the `/usr/bin/security` call; unknown-op arm panics.
- **Fix:** Pipe the secret over stdin (`security … -w` from `Stdio::piped()`); return `Result` from `args_for` or make the op an enum.
- **Effort:** S
- **Grade lift:** B− → B−

#### B7 — Tests absent exactly where the risk is
- **Where:** `src-tauri/src/sleeper.rs` (0 tests), `season.rs` (0), `board.rs` (0), `projections.rs` (0), `commands_draft.rs`/`commands_season.rs` (0), `engine.rs` (2 for a 16 KB module)
- **What's wrong:** The wire-parsing and view-assembly layers — where a Sleeper field rename silently zeroes the board — have no direct tests.
- **Fix:** Fixture-deserialize tests for `sleeper.rs`; golden test of `build_season_view` against `public/dev-season-fixture.json`; temp-dir table tests for `projections.rs` stale-fallback branches. (Tracked as the D-category coverage push.)
- **Effort:** M
- **Grade lift:** B− → B

---

## C — Frontend Quality — B

Unusually disciplined for hobby scale: small single-purpose components, `useSyncExternalStore` stores (`avatars.ts`, `zoom.ts`) instead of prop-threading, consistent real-button semantics, fully strict TypeScript with zero `any`/`@ts-ignore`, and every `localStorage` access try/caught. Held back by ARIA that is applied but often incorrect, a modal with no focus management, and ~2,900 lines of unscoped global CSS with cross-file selector coupling.

#### C1 — `aria-sort` on a `<button>` is invalid and announces nothing
- **Where:** `src/components/bits.tsx:251`; consumers `Board.tsx:194`, `SeasonTabs.tsx:71`
- **What's wrong:** `aria-sort` is only valid on `columnheader`/`rowheader`; on a plain button in a div-grid, screen readers get no sort state.
- **Fix:** Wrap header rows in `role="row"` with `<span role="columnheader" aria-sort={...}>` around each `SortHead`, or move boards to `role="grid"` with proper cells.
- **Effort:** S
- **Grade lift:** B → B+

#### C2 — Tablist is half-built with no keyboard model
- **Where:** `src/components/SeasonScreen.tsx:63-101`
- **What's wrong:** `role="tablist"`/`tab`/`aria-selected` present, but no `role="tabpanel"`, no `aria-controls`/`id` pairs, no arrow-key navigation, no roving tabindex.
- **Fix:** Add id/controls pairs, wrap panels in `role="tabpanel"`, roving `tabIndex`, ArrowLeft/Right/Home/End handler.
- **Effort:** S
- **Grade lift:** B → B+

#### C3 — Modal dialogs have no focus trap or focus restore
- **Where:** `src/components/bits.tsx:68-89` (`ZoomLayer`), `Overlays.tsx:29` (`ConfirmDialog`)
- **What's wrong:** `role="dialog" aria-modal="true"` but focus never moves in, Tab isn't trapped, and focus isn't returned to the opener.
- **Fix:** On open store `document.activeElement`, focus the close button, bound Tab cycling to the figure, restore on close.
- **Effort:** S
- **Grade lift:** B → B+

#### C4 — `ordinal()` duplicated, and the copies disagree
- **Where:** `src/App.tsx:455-459` vs `src/components/SeasonTabs.tsx:291-296`
- **What's wrong:** Two implementations of the same suffix logic; one guard is dead code — a maintenance trap.
- **Fix:** Export a single `ordinal()` from `format.ts`; delete both copies.
- **Effort:** S
- **Grade lift:** B → B

#### C5 — 16+ `useState` hooks in `App.tsx`, several feeding one child
- **Where:** `src/App.tsx:39-68`, `settingsRows` at `:320-375`
- **What's wrong:** `chime`/`polling`/`settingsOpen`/`chatOpen`/`preference` exist only to build `settingsRows`, which is rebuilt on every 3-second draft tick and pushed through 8 `Header` props.
- **Fix:** Move theme/chime/settings-menu state into a `useSyncExternalStore` module beside `avatars.ts`; `Header` subscribes directly. (Overlaps A6.)
- **Effort:** M
- **Grade lift:** B → B+

#### C6 — Season error state renders as a loading state
- **Where:** `src/App.tsx:415-418` (render), `:193` (rejection path)
- **What's wrong:** When `loadSeason` rejects, the error string is shown inside `.season-loading` with no retry affordance; recovery requires finding Settings → Refresh.
- **Fix:** Branch on `seasonError !== null` first; render an error block with "Try again" calling `api.loadSeason(true)`, mirroring `LaunchScreen.onRetry`.
- **Effort:** S
- **Grade lift:** B → B+

#### C7 — Ten unscoped global stylesheets with cross-file selector coupling
- **Where:** imports at `src/App.tsx:21-30`; e.g. `.board-row` in `board.css:58` vs `.board-row .avatar` in `components.css:486`
- **What's wrong:** Nothing is scoped; any new `.chip`/`.bar` class silently collides across files split by screen, not component.
- **Fix:** Keep `theme.css` as token/atom layer; convert component sheets to CSS Modules (zero-config in Vite), imported by their owning component.
- **Effort:** L
- **Grade lift:** B → B+

---

## D — Testing & Reliability — B−

What exists is the good kind: 104 Rust tests that are property-flavored (lineup fill order, odds monotonicity/determinism, trade one-sidedness — `season_lineup.rs:250-434`, `season_odds.rs:275-332`, `season_trades.rs:165-245`), a full-draft invariant simulation (`tests/simulation.rs:149`), and 81 behavior-driven Testing Library tests that mock only the IPC boundary. But measured coverage tells the honest story: **Rust 57.3% lines, frontend 71.8% lines**, and the untested set is load-bearing — `sleeper.rs` at 12% (every wire type in the app), `season.rs` at 0% (the 400-line view assembly), `engine.rs` at 36%, `api.ts` at 18% [FE], `SeasonTabs.tsx` at 44% [FE]. No coverage gate exists on either side, so this can only drift down.

#### D1 — [BE] Wire-parsing layer (`sleeper.rs`) has zero direct tests
- **Where:** `src-tauri/src/sleeper.rs:145-189` (`PlayerMeta`/`ProjectionRow`), `:49-55` (`last_regular_week`)
- **What's wrong:** 330 lines of `#[serde(default)]` deserialization against an undocumented endpoint; a Sleeper field rename silently yields `None`/`0.0` and mis-scores the entire board.
- **Fix:** Checked-in raw-response fixture; assert `stat("pass_yd")`, `adp_ppr`, bye-week `opponent: None`; three-case test for `last_regular_week`.
- **Effort:** S
- **Grade lift:** B− → B

#### D2 — [BE] View assembly (`season.rs`, `season_view_parts.rs`, `engine.rs::assemble`) untested
- **Where:** `src-tauri/src/season.rs` (0%), `season_view_parts.rs` (0%), `engine.rs:295` (`assemble`, incl. warning accumulation `:327,:342`)
- **What's wrong:** The largest single behavior in the app — assembling every screen — is exercised only indirectly.
- **Fix:** Golden test of `build_season_view` from constructed inputs; `assemble` test asserting warnings surface instead of failing the load.
- **Effort:** M
- **Grade lift:** B− → B+

#### D3 — [FE] `api.ts` at 18% — the entire IPC surface
- **Where:** `src/api.ts` (17.9% statements at audit)
- **What's wrong:** Only `validateDraftView` was tested; command routing, event validation, and the browser-fixture arm were dark.
- **Fix:** Mock `invoke`/`listen`; assert command names/args per wrapper, payload validation on events, fixture caching and read-only errors in the browser arm.
- **Effort:** S
- **Grade lift:** B− → B

#### D4 — [FE] `SeasonTabs.tsx` 44%, `Chat.tsx` 54% despite having test files
- **Where:** `src/components/SeasonTabs.tsx`, `src/components/Chat.tsx`
- **What's wrong:** Tab-switching branches, standings sorting, LastSeason rendering, and most Chat interaction paths unexercised.
- **Fix:** Per-tab render assertions, sort toggling, pending-trade badge, ordinal edge cases; Chat send/error/settings flows with a mocked api.
- **Effort:** S
- **Grade lift:** B− → B

#### D5 — No coverage gate on either side
- **Where:** `vitest.config.ts` (no `coverage.thresholds`), `package.json:19` (`verify` lacks coverage), CI has no `cargo llvm-cov`
- **What's wrong:** Coverage can silently regress; it took this audit to notice 57%/72%.
- **Fix:** `coverage.thresholds` in vitest at the achieved level; add `cargo llvm-cov --fail-under-lines <N>` to CI (locally optional — it doubles test time).
- **Effort:** S
- **Grade lift:** B− → B

#### D6 — [BE] Poll loops untestable and untested
- **Where:** `src-tauri/src/commands_draft.rs:207-305`, `commands_season.rs:82-142`
- **What's wrong:** Change detection, error aggregation, and emit gating — the logic that runs all draft night — has zero tests because it's welded to Tauri `State`.
- **Fix:** Same refactor as A3; then table-test `poll_once`.
- **Effort:** M
- **Grade lift:** B− → B

---

## E — Security — A−

Unusually disciplined for a local-first desktop app. Minimal Tauri capability set (`capabilities/default.json:7-10` — no fs/shell/http exposed to the webview). `headshots.rs:32-47` refuses everything but a bare hex hash on a hardcoded `sleepercdn.com` base or an `uploads/` URL with a hex stem — `https://evil.example/x.jpg` and traversal are refused with a test proving it (`headshots.rs:244-248`). All fetched hosts are compile-time constants. No secrets in the repo; `npm audit` clean (0 vulns / 319 deps). Key storage uses absolute-path `/usr/bin/security` with argv arrays; the chat CLI is spawned with `--tools ""` and prompt over stdin. Remaining gaps are defense-in-depth.

#### E1 — CSP is disabled entirely
- **Where:** `src-tauri/tauri.conf.json:24` (`"csp": null`)
- **What's wrong:** The webview may load script/image/connect from any origin while rendering remote-derived strings (team/player names).
- **Fix:** `"csp": "default-src 'self'; img-src 'self' data: asset: http://asset.localhost; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost"` — the data-URL headshot design already accommodates it.
- **Effort:** S
- **Grade lift:** A− → A

#### E2 — `extract_id` falls through to raw input — a latent SSRF-path primitive
- **Where:** `src-tauri/src/commands_draft.rs:11-18`; reaches `format!("{BASE}/league/{league_id}")` at `sleeper.rs:282`
- **What's wrong:** With no ≥15-digit run, `input.trim()` passes verbatim; a pasted `../../projections/nfl/2025` normalizes out of `/v1/`. Harmless today (same host, public GETs), one refactor from mattering.
- **Fix:** Reject unless all-ASCII-digits: `Err("that doesn't look like a Sleeper ID")`.
- **Effort:** S
- **Grade lift:** A− → A−

#### E3 — One cache filename skips the sanitizer
- **Where:** `src-tauri/src/season_history.rs:132-133` vs the filtered versions at `engine.rs:152-157`, `season_engine.rs:81-85`
- **What's wrong:** `history_name` interpolates `league_id` unsanitized — inconsistency is exactly how a traversal lands later.
- **Fix:** Extract one `fn safe_key(&str)` in `engine.rs`; call from all three sites.
- **Effort:** S
- **Grade lift:** A− → A−

#### E4 — Raw username interpolated into a URL path via bare `reqwest::get`
- **Where:** `src-tauri/src/commands_draft.rs:60-61`
- **What's wrong:** Skips pooled-client timeouts/user-agent and does no encoding/validation of the username.
- **Fix:** Validate `[A-Za-z0-9_]{1,32}` (Sleeper's own rule) and route through `state.engine.client` (same change as A4).
- **Effort:** S
- **Grade lift:** A− → A−

#### E5 — Chat CLI discovery trusts `PATH`
- **Where:** `src-tauri/src/chat_cli.rs:23-34` (`find_cli`)
- **What's wrong:** Executes the first `claude` on `PATH` with no ownership check; low risk on a single-user Mac.
- **Fix:** Prefer the known absolute paths; require the PATH fallback to live outside world-writable dirs.
- **Effort:** S
- **Grade lift:** A− → A−

---

## F — Dependencies & Tech Currency — A−

Current and unusually lean: 4 production npm deps, 6 direct Cargo deps, all at or near latest (React 19.2, Vite 7.3, Vitest 4.1, ESLint 10.9, TS 5.8, Tauri 2.11, Tokio 1.53, reqwest 0.12). Both lockfiles committed; CI runs `npm ci` against the full verify gate. `npm audit`: zero vulnerabilities. Hand-rolling base64 (`headshots.rs:64-88`, tested against RFC vectors) instead of adding a dep was a defensible call.

#### F1 — Two major versions of reqwest in the binary
- **Where:** `src-tauri/Cargo.lock` (0.12.28 direct + 0.13.4 transitive), `Cargo.toml:23`
- **What's wrong:** Two TLS/HTTP stacks compile into the binary.
- **Fix:** Bump the direct dep to `0.13` and re-lock, or confirm via `cargo tree -d` the duplicate is unavoidable.
- **Effort:** S
- **Grade lift:** A− → A−

#### F2 — Bare-major version ranges on the framework
- **Where:** `src-tauri/Cargo.toml:20-23`
- **What's wrong:** `tauri = "2"` means `cargo update` can jump minors unreviewed.
- **Fix:** Pin tested minors (`tauri = "2.11"`, `reqwest = "0.12.28"`) so upgrades are explicit commits.
- **Effort:** S
- **Grade lift:** A− → A−

#### F3 — No supply-chain job in CI
- **Where:** `.github/workflows/verify.yml`
- **What's wrong:** RustSec advisories on transitives would go unnoticed; `npm audit` never runs in CI.
- **Fix:** Add `cargo audit` + `npm audit --audit-level=high` as a step or weekly scheduled job.
- **Effort:** S
- **Grade lift:** A− → A

#### F4 — `coverage/` not gitignored
- **Where:** `.gitignore` (missing entry); `coverage/` appears untracked after test runs
- **What's wrong:** Generated HTML will eventually get committed.
- **Fix:** Add `coverage` to `.gitignore` alongside `dist`.
- **Effort:** S
- **Grade lift:** A− → A−

---

## G — Performance & Scalability — C+

The caching design is good (`avatars.ts:46-57` memoizes in-flight promises; backend gates emissions on real score changes). But the season poll does heroic amounts of discarded work: every 30 seconds `build_season_view` runs a 4,000-iteration Monte Carlo, ~1,600 lineup solves, and a triple-nested trade search — to redraw a scoreboard whose totals moved 0.1 points. The frontend has zero `React.memo` and a 200-row board that fully re-renders on every 3-second tick.

#### G1 — Rival candidate lists rebuilt inside the free-agent loop, ~780×/poll
- **Where:** `src-tauri/src/season_moves.rs:94-97` (`CANDIDATE_POOL=60` × ~13 rivals)
- **What's wrong:** Identical `Vec<Candidate>` reconstructed per (agent × rival) pair, each triggering two lineup solves.
- **Fix:** Hoist `rival_pools: Vec<(Vec<Candidate>, f64)>` (pool + baseline) above the loop; index in.
- **Effort:** S
- **Grade lift:** C+ → B− (60× fewer constructions, half the solves)

#### G2 — `marginal_gain` recomputes a baseline the caller has
- **Where:** `src-tauri/src/season_moves.rs:63-64` vs baseline at `:79`
- **What's wrong:** ~840 identical `lineup_total` recomputations per poll.
- **Fix:** Pass `baseline: f64` into `marginal_gain`.
- **Effort:** S
- **Grade lift:** C+ → B− (with G1)

#### G3 — Trade search: ~2,900 iterations × 2 full-roster clones each
- **Where:** `src-tauri/src/season_trades.rs:76-86` (`total_after_swap` clones), `MAX_TRADES=4` truncate at `:133`
- **What's wrong:** On the order of a million `String` allocations per poll to keep 4 results.
- **Fix:** Prune pairs where `theirs.points - ours.points < MIN_EDGE` at the same position before solving; consider `Arc<str>` for `Candidate` fields.
- **Effort:** M
- **Grade lift:** C+ → B−

#### G4 — Odds simulation runs through `HashMap` in the hot loop
- **Where:** `src-tauri/src/season_odds.rs:142-186` (4,000 sims × ~90 games; three HashMaps + a re-sort per sim)
- **What's wrong:** ~4-5M hash lookups per call for what could be flat vector indexing.
- **Fix:** Map teams to 0..n once; replace `wins`/`points` maps with `Vec<f64>` indexed by slot.
- **Effort:** S
- **Grade lift:** C+ → B− (~10× on the sim)

#### G5 — The full season view is rebuilt every 30s when only live scores changed
- **Where:** `src-tauri/src/commands_season.rs:116-133` (poll refreshes live only; emit-gate at `:133` suppresses emit, not computation), `season.rs:244,313,343`
- **What's wrong:** G1-G4's work — waivers, trades, odds — cannot change from a touchdown, yet is recomputed and thrown away every tick.
- **Fix:** Split the view into cheap live + expensive analysis sections; cache analysis in `AppState` on `load_season`/`refresh_season`; merge on tick.
- **Effort:** M
- **Grade lift:** C+ → B (makes G1-G4 mostly moot on the poll path)

#### G6 — 200 unmemoized board rows re-render on every 3s tick
- **Where:** `src/components/Board.tsx:243-284`; `Headshot` subscriptions via `bits.tsx:105-115`
- **What's wrong:** ~400 store-subscribing leaves churn per `draft-updated`; "Show all" unbounds it entirely.
- **Fix:** `memo`-wrapped `BoardRow`, `useCallback` the draft handler, pass `avatarMode` down instead of 400 subscriptions; window the "Show all" path.
- **Effort:** M
- **Grade lift:** C+ → B−

#### G7 — Board filter+sort re-runs every tick with an O(n log n × getter) comparator
- **Where:** `src/components/Board.tsx:131-142`
- **What's wrong:** `view.available` is a fresh identity each event, so the memo never hits; `column.value()` runs per comparison.
- **Fix:** Precompute sort keys, then sort; memoize on `total_picks_made` + length instead of array identity.
- **Effort:** S
- **Grade lift:** C+ → C+

#### G8 — Avatars cross the IPC bridge as base64 data URLs; no build target set
- **Where:** `src-tauri/src/headshots.rs:90-92`; `vite.config.ts` (no `build.target`/`sourcemap`/analyze script)
- **What's wrong:** ~33% size penalty + JSON round-trip per image; nothing would catch a heavy dep being added.
- **Fix:** Serve cached files via Tauri's `asset:` protocol; set `build: { target: "es2021", sourcemap: true }`; add `rollup-plugin-visualizer` behind `npm run analyze`.
- **Effort:** M
- **Grade lift:** C+ → B−

---

## H — Documentation & Onboarding — C+

`draft-assistant/README.md` is genuinely well-written for what it covers (domain model, run/build/dump commands, cache TTLs, browser-preview fixtures), and 41 of 43 Rust files carry `//!` module headers. The fatal gap is drift: the README describes a draft-only app while half the tree — 20 `season_*.rs` files, the chat feature, and the entire Season frontend — is never mentioned. No root README, no `docs/`.

#### H1 — README layout block covers 9 of 43 Rust files
- **Where:** `draft-assistant/README.md:77-92`
- **What's wrong:** `season*.rs`, `chat*.rs`, `commands_*.rs`, `state.rs`, `secrets.rs`, `roster.rs`, `projections.rs` are invisible to a newcomer.
- **Fix:** Regenerate the layout block; add "Season screen" and "Ask Claude" architecture paragraphs parallel to the draft one.
- **Effort:** S
- **Grade lift:** C+ → B−

#### H2 — Season fixture and `dump_season` undocumented
- **Where:** `draft-assistant/README.md:61-65` vs `src/api.ts:124-127` and `src-tauri/src/bin/dump_season.rs`
- **What's wrong:** Someone regenerating fixtures will silently ship a stale season screen.
- **Fix:** Document `cargo run --bin dump_season -- <league_id> [username] [out.json]` beside `dump_state`; note both fixtures regenerate together.
- **Effort:** S
- **Grade lift:** C+ → B−

#### H3 — No root README
- **Where:** repo root (ships `TRACKER.md`, `draft-assistant-prompt.md`, two grade reports, no orientation)
- **What's wrong:** A clone lands on four ambiguous markdown files; nothing says the app lives in `draft-assistant/`.
- **Fix:** ~15-line root README: what this is, `cd draft-assistant && npm install && npm run tauri dev`, what TRACKER.md is.
- **Effort:** S
- **Grade lift:** C+ → B−

#### H4 — No CLAUDE.md codifying the project's hard conventions
- **Where:** repo root / `draft-assistant/`
- **What's wrong:** The 500-LOC cap, the verify gate, and fixture-regeneration rules live only in prose or a contributor's head.
- **Fix:** Add `CLAUDE.md` codifying LOC cap, verify gate, fixture rules.
- **Effort:** S
- **Grade lift:** C+ → B−

#### H5 — Ask Claude has zero setup documentation
- **Where:** `draft-assistant/README.md` (absent); auth routes in `src-tauri/src/chat_cli.rs:25-31` and `secrets.rs`
- **What's wrong:** The feature appears broken to anyone without the CLI or an API key.
- **Fix:** "Ask Claude" README section covering both auth routes and where the key is stored.
- **Effort:** S
- **Grade lift:** C+ → B−

---

## I — Developer Experience & Tooling — B−

The core loop is real: one `verify` script chains LOC cap → fmt → tsc → build → both test suites → eslint `--max-warnings=0` → clippy `-D warnings`, and CI (`verify.yml:36-37`) runs exactly that, so local and CI can't diverge. The browser-fixture dev loop (`api.ts:116-137`) is a genuine strength — full-fidelity UI with no Rust compile. Weak at the edges: no Rust caching in CI on a macOS runner, no TS formatter, ESLint scoped to `src/` only, nothing pinning toolchains locally.

#### I1 — CI rebuilds the full Tauri dependency graph every run on macOS minutes
- **Where:** `.github/workflows/verify.yml:25-31` (npm cached, Rust not)
- **What's wrong:** 27 dependency trees compiled from scratch per run at 10× minute billing; realistically 10-20 min per PR.
- **Fix:** `Swatinem/rust-cache@v2` with `workspaces: draft-assistant/src-tauri` after the toolchain step.
- **Effort:** S
- **Grade lift:** B− → B+

#### I2 — `verify` runs the fastest-failing checks last
- **Where:** `draft-assistant/package.json:19`
- **What's wrong:** A one-character clippy violation costs the full test compile before surfacing.
- **Fix:** Reorder to loc → fmt → lint → typecheck → test → build, or add a `verify:fast` subset.
- **Effort:** S
- **Grade lift:** B− → B

#### I3 — No formatter for 37 TS files and 9 CSS files
- **Where:** `draft-assistant/package.json:11` (`format:check` is cargo-fmt only)
- **What's wrong:** Frontend formatting enforced by nothing but discipline.
- **Fix:** Add prettier + `.prettierrc`; chain `prettier --check src scripts` into `format:check`.
- **Effort:** S
- **Grade lift:** B− → B

#### I4 — Config files unlinted and untypechecked
- **Where:** `eslint.config.js:11` (`files: ["src/**"]`), `tsconfig.json:33` (`include: ["src"]`)
- **What's wrong:** `scripts/check-loc.mjs`, `vite.config.ts`, `vitest.config.ts`, and the eslint config itself are checked by nothing.
- **Fix:** Widen the ESLint glob (+`globals.node`); use `tsc --build --noEmit` so the node tsconfig is checked.
- **Effort:** S
- **Grade lift:** B− → B−

#### I5 — No local toolchain pinning
- **Where:** CI pins stable Rust + Node 22; no `rust-toolchain.toml`, `.nvmrc`, or `engines`
- **What's wrong:** A dev on Node 20 or newer Rust gets lints CI didn't have, or vice-versa.
- **Fix:** Add `rust-toolchain.toml` (channel + rustfmt/clippy components) and `.nvmrc`; have CI read them.
- **Effort:** S
- **Grade lift:** B− → B−

#### I6 — No pre-commit hook
- **Where:** `.git/hooks/` (samples only); no husky/lefthook
- **What's wrong:** Every mistake round-trips through a 10-20 min CI run.
- **Fix:** `.githooks/pre-commit` running the sub-15-second subset (loc + eslint + tsc) with `core.hooksPath` documented.
- **Effort:** S
- **Grade lift:** B− → B−
