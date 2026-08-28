# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-28, 12:35–12:45 PDT — **draft day, ~4¼ hours to first pick**
(earlier today: `.claude/grade-report-2026-08-28-1100.md`, `-0800.md`, `-morning.md`; yesterday `-2026-08-27.md`. This run was asked to focus on **Testing, Code Quality and App Function**; A–D and I were re-sampled from scratch, E and F had their audit tooling re-run, G and H are carried forward unchanged and say so.)
**Stack:** Tauri 2 desktop app — Rust/Tokio core (`src-tauri/`, 5,509 LOC src + 12 integration/property test files) + React 19 / TypeScript-strict / Vite 7 frontend (`src/`, 2,355 LOC TS/TSX + 2,068 LOC tests), Bun. Ask Claude via the local `claude` CLI, streamed.

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | A− *(confirmed; was provisional at 12:05)* | 1 |
| B | Backend Quality | B+ | 5 |
| C | Frontend Quality | B+ | 5 |
| D | Testing & Reliability | B+ *(down from the provisional A−: the headline number was flattering — see D1)* | 5 |
| E | Security | B+ | 3 |
| F | Dependencies & Tech Currency | B− | 3 |
| G | Performance & Scalability | B+ *(carried forward, not re-sampled)* | 3 |
| H | Documentation & Onboarding | B+ *(carried forward, not re-sampled)* | 2 |
| I | Developer Experience & Tooling | B *(down from B+: `verify` fails after `coverage`, and the dev watcher reloads the live window — I1, I2)* | 4 |
| **Overall** | | **B+** | **31** |

**Top 5 highest-leverage fixes:** I2, I1, D1, D2, C1 — **all five plus B2 executed at 13:30** (TRACKER rows 38–43). Remaining highest-leverage: B1, D3, A1, E1, D5.

---

## Fresh evidence this run (12:35–12:42)

| Check | Result |
|---|---|
| `bun run verify` | **exit 1** — every stage green (LOC cap, `cargo fmt`, `tsc`, Vite build, **73 Vitest**, **140 Rust**, **16 Playwright** in 4.2 s) until the last one: `eslint . --max-warnings=0` linted `coverage/*.js` (the report `coverage:frontend` had just written) and found 3 warnings. `lint:rust` never ran in that chain; run alone, `cargo clippy --all-targets --all-features -D warnings` → **0 warnings**. CI on the branch is green because the runner never runs `coverage` first. |
| Coverage, Rust (`cargo llvm-cov --all-targets`) | **92.0 % lines** (3,724 lines, 298 missed). Per file: `manual.rs`/`roster.rs` 100 %, `draft.rs` 99 %, `engine.rs` 98 %, `chat/*` 93–97 %, `app.rs` 95 %, `dump_state.rs` 95 %, `board.rs` **84.6 %**, `desktop.rs` **0 %** (129 lines of Tauri glue), `main.rs` 0 %. |
| Coverage, frontend (`vitest run --coverage`) | **93.4 % lines** *as reported* — but the table has **no row for `api.ts` (274 lines) or `Markdown.tsx`**. Both are imported with `await import()` after `vi.resetModules()` and the v8 provider drops them; a targeted run (`vitest run src/api --coverage`) confirms: the summary counts 163 lines, all from `chatMarkdown.tsx`. The 11:00 report's "`api.ts` 100 %" was not supported by the tooling. |
| Quality signals | `#[allow(…)]` in `src-tauri/src`: **0**. `eslint-disable`: 0. `@ts-ignore`/`as any`: 0. `#[ignore]`, `.skip`, `.only`: 0. `TODO/FIXME`: 0. Every first-party file ≤ 466 lines (`recommend.rs`), cap 500 enforced. |
| Audits | `cargo audit`: 0 vulnerabilities, the same 17 unmaintained Linux-only crates (519 deps). `bun audit`: clean. |
| **App function, live** | `dump_state` against the real league from a copy of the app's cache: **1.28 s**, schema 1.3, `status pre_draft`, pick 1 / round 1, `my_slot 2`, `my_next_picks [2, 27, 30, 55, 58, 83, …]`, Gainwell R10 + Stafford R14 on the roster, 392 of 419 available, `warnings: []`, poll health clean. Top recommendation Jahmyr Gibbs (tier 1, 74 % survival to 27). Identical to the 10:51 and 08:35 dumps. |
| Live feed detail | Sleeper's picks feed now shows **27 picks, of which only 24 carry `is_keeper: true`** — Achane (82), Gainwell (139) and Stafford (195) have `is_keeper: null`. The app is unaffected (it keys keepers on pick number, not the flag: `view::next_open_pick`), but B2 below must not rely on the flag. |
| The window on screen | `target/debug/draft-assistant` under **`tauri dev`**, up 36 min, 128 MB RSS, 0 % CPU idle, two TLS connections to Sleeper open, log clean of panics/errors. **But**: Vite's watcher ignores only `src-tauri/`, so this run's `coverage/` and `playwright-report/` writes each triggered `[vite] page reload` in the live window (12:37:44, 12:38:00). A reload keeps the board (state lives in Rust) but **drops the chat conversation** — `turns` is React state; only prefs persist (`Chat.tsx:46`, `chatOptions.ts:30-45`). |

---

## Post-report addendum — 16:15, driving the real window

Accessibility and screen-recording permission arrived, so the real Tauri
window was driven directly (CGEvent clicks + screenshots) rather than
through the browser preview. That found what no test had:

- **`reconcile_manual_picks` silently deleted every manual pick within 3 s**
  (`engine.rs`). It kept only picks *beyond the highest* API pick, a rule
  `view::merged_picks` was fixed away from months ago; this league's feed
  opens with keepers up to pick 195, so the entire offline fallback was
  dead in exactly the league it was written for. **Fixed** — keyed on pick
  number and player, with a regression test naming the live shape.
- **The poll never told the UI**, because the fingerprint covers only the
  feed (`app.rs`). The board rendered a pick the backend had dropped and
  Undo said "no manual picks to undo". **Fixed** — a reconcile that changes
  anything forces an emit.
- A reopened chat session's label read "not saved yet". **Fixed.**

This is the strongest argument yet for D3 (a smoke test that boots the
shell): both bugs sat behind the one seam the 92 %/91 % suites do not
cross, and one of them would have cost picks tonight.

---

## Draft-day readiness

**Verdict: still yes. Green, with one new yellow that is operational, not code.**

| | Status | Evidence |
|---|---|---|
| Correctness on your league | 🟢 | Live dump 12:41 above — same answers as three earlier dumps today; keeper handling holds even with three un-flagged keepers on the feed |
| Build on screen | 🟢 | Running the tested commit (`1748d8e` code; `f24e436` is docs-only); clippy 0, 229 tests green, CI green (run 33202446623, 5m21s) |
| Caches | 🟢 | All three fetched 12:04 → projections good until **18:04**, players until tomorrow |
| Failure behaviour | 🟢 | Unchanged and now tested: stale-cache fallback with age warnings, poll failure counting and recovery, hung Claude killed, cut-off answer kept (`tests/engine_cache.rs`, `tests/app_core.rs`, `chat/cli.rs` tests) |
| **The dev-mode window** | 🟡 | Anything that writes into the worktree while it runs — `verify`, `coverage`, Playwright, a `git checkout`, the dogfood scripts copying into `public/` — reloads the window and wipes the chat panel's conversation. **Tonight: do not run anything in this worktree while the app is open**, or (five-minute fix, I2) add the ignore list and relaunch before 17:00. A packaged `tauri build` binary has no watcher at all. |
| Eyes on the streaming path | 🟡 | Unchanged from 11:00 — one throwaway question in the real window is the only way to see the `Channel` deliver |

---

## A — Architecture & Design — A−

Confirmed on a full read of the two files the 12:05 extraction produced. `desktop.rs` is 169 lines and every command body is one `state.core.<method>(…)` call; the only logic left is the interval clamp, the `Channel` closure (with the closed-channel case reasoned in a comment), and the `PollEvent → app.emit` match (`desktop.rs:80-96`, `:111-131`). `app.rs` holds the state, the commands and the poll loop with no Tauri type in it, which is exactly why `tests/app_core.rs` could drive the poll state machine with a `Vec`-collecting closure. `chat/` is layered `stream` → `cli` → `mod` and streams through `&mut dyn FnMut(&str)`. `#[allow]` count in the crate: zero. The one structural item left is the hand-mirrored type contract.

#### A1 — Generate `types.ts` from the Rust structs instead of mirroring by hand
- **Where:** `src/types.ts:1-196`; sources `src-tauri/src/view.rs`, `board.rs`, `draft.rs`, `recommend.rs`, `chat/mod.rs`, `chat/stream.rs`
- **What's wrong:** Fourteen interfaces transcribed by hand; each schema change this week needed lockstep edits in four places, and the only guard is the `schema_version` string compare in `api.ts`, which catches a forgotten bump, not a drift.
- **Fix:** `ts-rs` as a dev-dependency, derive `TS` on the view and chat structs, emit `src/types.generated.ts` from `cargo test`, fail `verify` on a diff.
- **Effort:** M
- **Grade lift:** A− → A (the Rust↔TS boundary becomes compiler-checked)

---

## B — Backend Quality — B+

Re-sampled: clippy is clean with `-D warnings` across all targets and features, no `#[allow]`, no `unwrap` outside tests on the request paths I read (`app.rs`, `desktop.rs`, `engine.rs`), and the live dump reproduces the same board and roster as every dump today in 1.3 s from cache. The uncovered lines in `app.rs` are exactly the failure branches (`:173-174` manual-pick rollback on a failed save is covered by a test; `:231-236`, `:243` the poll loop's save-failure and refresh-failure arms are not) — small and honest. What keeps it at B+ is unchanged: no log file, `is_keeper` never surfaced (and today's feed shows the flag is not even reliable), the mock-slot fallback, and two small validation nits.

#### B1 — Write a log file the user can actually find
- **Where:** `src-tauri/src/engine.rs:182` (the crate's only `eprintln!`), `desktop.rs:122-126` (`.ok()` discards emit failures), `:87-91` (closed `Channel` ignored by design), `run()` registers no log plugin
- **What's wrong:** A packaged `.app` sends stderr nowhere; today's dev instance sends it to `/private/tmp/tauri-dev-relaunch2.log`, which contains only Vite's reload notices. If the board freezes at pick 40 tonight there is nothing to read afterwards.
- **Fix:** `tauri-plugin-log` to `~/Library/Logs/com.justin.draft-assistant/`; info on each emit (`seq`, pick count), cache hit/fallback with age, chat spawn/first-token/exit; warn on every poll error, emit failure and channel send failure. ~12 call sites.
- **Effort:** S
- **Grade lift:** B+ → A− (post-mortems become possible)

#### ~~B2 — Surface keepers in the view — by pick presence, not by the `is_keeper` flag~~ ✓ done 2026-08-28 13:30 (`view::keeper_pick_nos`, `LoadedLeague.keeper_pick_nos`, `RosterEntry.is_keeper`, keeper tag in the roster)
- **Where:** `src-tauri/src/sleeper.rs` (`Pick` has no `is_keeper`), `view.rs` (`RecentPick`, roster entries), `draft.rs` (`RosterEntry`)
- **What's wrong:** The app handles keepers correctly but never says "keeper". **New today:** the live feed carries 27 keeper picks and only 24 have `is_keeper: true` — both of yours are `null` — so a fix that reads the flag would label your own keepers wrong.
- **Fix:** Deserialize `is_keeper` as `Option<bool>`, but derive `keeper = is_keeper == Some(true) || pick was present while status == "pre_draft"` (record the pre-draft pick set at load). Carry through `RosterEntry` and `RecentPick`, render a "keeper" tag; it reaches the prompt for free via the state JSON.
- **Effort:** S
- **Grade lift:** B+ → A− (the data model stops hiding the draft's most distinctive fact, and does it with data that is actually reliable)

#### B3 — Gate the mock-draft slot fallback on an explicit flag
- **Where:** `src-tauri/src/view.rs` (`user_names.is_empty()` fallback), `engine.rs` (`load_draft_only`)
- **What's wrong:** Carried. The "adopt the draft creator's slot" fallback fires whenever `user_names` is empty, which a transient `/league/{id}/users` failure also produces. Reachable only with `my_user_id` unresolved — not your config.
- **Fix:** `is_mock: bool` on `LoadedLeague`, set only in `load_draft_only`; gate on it; warn when the users fetch fails.
- **Effort:** S
- **Grade lift:** B+ → A− (a silently-wrong-team state becomes impossible in a real league)

#### B4 — Enforce the chat spend ceiling in the backend too
- **Where:** `src/components/Chat.tsx` (`overBudget`, panel-side only); `src-tauri/src/chat/mod.rs` (`ask` has no notion of spend); `bin/dump_state.rs` (unbounded `--ask` loop)
- **What's wrong:** Carried. The budget lives in the panel; the crate and the CLI will run as many calls as asked.
- **Fix:** Per-process running total in `chat::ask`, refuse past `DRAFT_ASSISTANT_CHAT_BUDGET_USD` (default 10).
- **Effort:** S
- **Grade lift:** B+ → B+ (defence in depth on cost)

#### B5 — Validate the username before it reaches a URL
- **Where:** `src-tauri/src/sleeper.rs` (`format!("user/{username}")`)
- **What's wrong:** Carried. Typed text interpolated raw into a path segment; self-inflicted only.
- **Fix:** Reject anything outside `[A-Za-z0-9_-]` with a clear message.
- **Effort:** S
- **Grade lift:** B+ → B+ (correctness nit)

---

## C — Frontend Quality — B+

Re-sampled. The code reads well: `format.ts` is three honest helpers with a comment explaining the `String(error)` prefix problem; `ConfirmDialog` uses a real `<dialog>` with `onCancel`, Escape and backdrop-click all routed to one handler and focus restored to the opener on close (`ConfirmDialog.tsx:40-60`); `loadPrefs` validates every field it reads from `localStorage` and falls back on any throw (`chatOptions.ts:28-41`). `App.tsx` is at 393 lines with the wiring for Board, Confirm, toast and Chat in one return (`:322-353`) — the least-covered file in the frontend (83 % lines, 72 % branches) precisely because that composition is only exercised end-to-end. New this run: the chat conversation does not survive a reload, which today's watcher finding turned from theoretical into observed.

#### ~~C1 — Persist the chat conversation across a reload~~ ✓ done 2026-08-28 13:30 (saved sessions: `chat/session.rs`, `ChatSessions.tsx`, restore on open, New chat)
- **Where:** `src/components/Chat.tsx:46` (`const [turns, setTurns] = useState<Turn[]>([])`); `chatOptions.ts:26-48` persists only `ChatPrefs`
- **What's wrong:** A window reload — dev watcher, error-boundary recovery, or a deliberate relaunch to pick up fresh projections — wipes every answer Claude has given tonight while the board comes straight back from the Rust side. Observed twice today at 12:37 and 12:38.
- **Fix:** Persist `turns` (and `session` cost) to `localStorage` under the draft ID, restore on mount, clear on "New chat". The `asOfPick` stamps already make restored answers self-describing.
- **Effort:** S
- **Grade lift:** B+ → A− (the panel's state becomes as durable as the board's)

#### C2 — Add a way back to Setup / a league switcher
- **Where:** `src/App.tsx` (Setup renders only when `view === null`); `AppConfig.leagues` is populated and never read by the UI
- **What's wrong:** Carried. Once a league loads there is no path back short of editing `config.json`.
- **Fix:** A league menu in the header listing `config.leagues` plus "Add another league…".
- **Effort:** M
- **Grade lift:** B+ → A− (removes the only dead end)

#### C3 — Extend the tier colour scale past five
- **Where:** `src/components/Board.tsx` (`tier-${Math.min(p.tier, 5)}`), `components.css`
- **What's wrong:** Carried. Tiers run to T17; every badge from T5 down is one colour.
- **Fix:** `tier-6`…`tier-8` steps clamped at 8, or three semantic buckets.
- **Effort:** S
- **Grade lift:** B+ → A− (the tier column carries signal below the top bands)

#### C4 — Show every team's roster
- **Where:** `src/components/Panels.tsx`; `view.rosters` is populated for 14 teams and rendered nowhere
- **What's wrong:** Carried. In a keeper league you cannot see who kept whom without leaving for Sleeper.
- **Fix:** A collapsed "All rosters" section rendering `view.rosters` as compact columns.
- **Effort:** M
- **Grade lift:** B+ → A− (closes the last reason to switch apps mid-draft)

#### C5 — Put auto-ask and budget in the settings summary line
- **Where:** `src/components/chatOptions.ts` (`describeOptions` reads `ChatOptions` only); `ChatSettings.tsx`
- **What's wrong:** Carried. The folded header says nothing about the two controls that change behaviour most tonight.
- **Fix:** Append "auto-ask on" / "$5 budget" to the summary; update the two pinned assertions.
- **Effort:** S
- **Grade lift:** B+ → B+ (discoverability)

---

## D — Testing & Reliability — B+

The suite is real and it is green: 140 Rust (7 unit modules + 12 integration files including the stub-Sleeper `engine_cache`, `app_core`, `dump_state_cli` suites and a proptest file), 73 Vitest, 16 Playwright, zero skipped/ignored, CI green five times today. Rust line coverage is a measured **92.0 %** with only the Tauri adapter at zero. Why this run takes the provisional A− back to B+: (1) the frontend headline **omits `api.ts`** — the entire IPC/browser bridge, 274 lines, with two test files — and `Markdown.tsx`, because the v8 provider drops modules re-imported after `vi.resetModules()`; the "93.4 %" is over 12 files, not 14, and the earlier "`api.ts` 100 %" claim had no tooling behind it; (2) `board.rs` is at 84.6 % because **the bye-week inference and per-game bonus grouping (`board.rs:93-121`) have never executed under a test** — `tests/fixtures/board_input.json` has zero weekly rows — and neither has the "no scored players" degradation (`:197-201`); (3) `verify` is not green on a laptop that has just run `coverage` (I1). The poll-loop tests are timing-based (25 ms interval, 120 ms settle, 5 s cap — `tests/app_core.rs:289-340`); acceptable, not flake-proof on a loaded runner.

#### ~~D1 — Make the frontend coverage number honest `[FE]`~~ ✓ done 2026-08-28 13:30 (`vitest.config.ts` coverage.include; true figure 91.4 %)
- **Where:** `vitest.config.ts` (no `coverage` block); `src/api.test.ts:16,46,78` and `src/components/Markdown.test.tsx` (`await import()` after `vi.resetModules()`)
- **What's wrong:** `api.ts` and `Markdown.tsx` are absent from the report, so the 93.4 % line is over a subset. A future regression in the bridge would not move the number at all.
- **Fix:** Add `coverage: { include: ["src/**/*.{ts,tsx}"], exclude: ["src/**/*.test.*", "src/test/**", "src/main.tsx", "src/types.ts", "src/vite-env.d.ts"], reportsDirectory: "coverage" }` so every source file is listed (at 0 % if untouched); then either import `./api` statically in the tests that can (the browser-branch tests) or accept the dynamic-import cases as instrumented-by-Playwright and say so in the README's Testing section. Re-run and record the true number.
- **Effort:** S
- **Grade lift:** B+ → B+ (the metric stops lying; the grade moves when D2 lands)

#### ~~D2 — Cover the weekly-projection path: bye inference and per-game bonuses `[BE]`~~ ✓ done 2026-08-28 13:30 (three tests in `tests/board_fixture.rs`; `board.rs` 94.6 %)
- **Where:** `src-tauri/src/board.rs:86-121` (uncovered), `:132-138`, `:197-201`; `src-tauri/tests/fixtures/board_input.json` (`season_rows` only, no weekly rows); `tests/board_fixture.rs`
- **What's wrong:** The code that decides each player's bye week — by counting opponent rows per team and taking the week with ≤ ¼ of the max — and that groups weekly stats for the bonus model runs only on live data. A stray traded-player row, a team with 17 games, or a weekly feed that arrives empty are all untested branches on the path that builds tonight's board.
- **Fix:** Add ~40 weekly rows to the fixture (two teams × 17 weeks, one with a stray row in the bye week, one player with `stats`), assert the bye for each team and the bonus expectation for the one player; a second case with all weekly rows missing asserts the "no scored players" warning and an empty board rather than a panic.
- **Effort:** S
- **Grade lift:** B+ → A− (the last untested data-shaping path is covered)

#### D3 — Add a smoke test that the desktop shell boots `[both]`
- **Where:** `src-tauri/src/desktop.rs` (129 lines at 0 %), `src/main.tsx`
- **What's wrong:** Carried, narrowed: everything below the glue is tested; "the window opens white" or "the channel never delivers" are still caught only by launching.
- **Fix:** A `--features desktop` test that builds `AppState`, calls the command functions directly against the stub server (they are plain `async fn`s taking `State<'_, AppState>` — construct via `tauri::test::mock_app()`), and asserts a `DraftView` and a `Channel` delivery.
- **Effort:** M
- **Grade lift:** B+ → A− (the boot path gets a floor)

#### D4 — Keep one Rust fixture honest about keepers `[BE]`
- **Where:** `src-tauri/tests/fixtures/board_input.json` (no keepers); `tests/keepers.rs` (hand-built league)
- **What's wrong:** Carried, and sharpened by today's feed: three real keeper picks arrive with `is_keeper: null`. No fixture has that shape.
- **Fix:** Capture a sanitised 30-pick slice of tonight's feed (both flag states) into `tests/fixtures/`, assert `next_open_pick`, roster assembly and the B2 keeper derivation against it.
- **Effort:** S
- **Grade lift:** B+ → B+ (real payload shape covers the feature that broke yesterday)

#### D5 — Take the wall clock out of the poll-loop tests `[BE]`
- **Where:** `src-tauri/tests/app_core.rs:289-305` (`wait_for` polls every 20 ms with a 5 s cap), `:333` (`sleep(120 ms)` to assert "no re-emit")
- **What's wrong:** "Nothing happened in 120 ms" is the assertion for "no change → no emit"; on a runner several times slower than this laptop (the README already notes CI needed a 20 s Vitest timeout) that window can hide a late emit or trip the 5 s cap.
- **Fix:** Use `tokio::time::pause()` + `advance()` with a `start_paused` test, or expose `poll_once` (already public) and assert emit counts per explicit poll rather than per elapsed time; keep one wall-clock test for `poll_loop`'s generation handoff.
- **Effort:** S
- **Grade lift:** B+ → B+ (removes the only timing-based assertions in the suite)

---

## E — Security — B+

Audit tooling re-run this session: `cargo audit` 0 vulnerabilities (same 17 unmaintained Linux-only crates), `bun audit` clean; no new inputs or surfaces since 11:00. Grade and items carried forward.

#### E1 — Set a Content Security Policy
- **Where:** `src-tauri/tauri.conf.json` (`"csp": null`)
- **What's wrong:** No second line of defence behind React's escaping and the HTML-free markdown renderer.
- **Fix:** `"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost https://api.sleeper.app; img-src 'self' data:"`; verify dev and packaged builds.
- **Effort:** S
- **Grade lift:** B+ → A−

#### E2 — Record why the 17 unmaintained-crate advisories are accepted
- **Where:** no `src-tauri/audit.toml`; one open Dependabot alert (`glib` RUSTSEC-2024-0429)
- **What's wrong:** All Linux-only Tauri deps absent from the macOS graph, but nothing in the repo says so.
- **Fix:** `audit.toml` ignoring those IDs with a reason each; `cargo audit` in `verify`; dismiss the alert with the same reason.
- **Effort:** S
- **Grade lift:** B+ → A−

#### E3 — Validate the username before it reaches a URL
- **Where:** as B5. **Fix:** as B5. **Effort:** S. **Grade lift:** B+ → B+

---

## F — Dependencies & Tech Currency — B−

Re-checked: audits clean, toolchain pinned (1.88.0, confirmed active for every cargo invocation this run). Items carried forward unchanged.

#### F1 — Take the three dev-dependency majors (vite 8, typescript 7, plugin-react 6) after the draft, one at a time with `verify` between. **Effort:** M. **Grade lift:** B− → B
#### F2 — Enable Dependabot version updates (`.github/dependabot.yml`, npm + cargo, weekly, grouped). **Effort:** S. **Grade lift:** B− → B
#### F3 — Pin Bun in CI: `.github/workflows/verify.yml:27` is `bun-version: latest`; set `1.3.14` and `engines.bun`. **Effort:** S. **Grade lift:** B− → B−

---

## G — Performance & Scalability — B+ *(carried forward from 11:00, not re-sampled this run)*

One fresh data point only: the live dump from cache builds the full 419-player board in 1.28 s wall / 0.75 s CPU, and the running window sits at 128 MB RSS and 0 % CPU between polls after 36 minutes.

#### G1 — Stream or narrow the 18 MB weekly projections download (`sleeper.rs`). **Effort:** M. **Grade lift:** B+ → A−
#### G2 — Memoize `build_view` on `poll_fingerprint` — headroom only. **Effort:** M. **Grade lift:** B+ → B+
#### G3 — Trim `scoring_settings`, `data_health`, `replacement_demand` from the model's state JSON (`chat/prompt.rs`). **Effort:** S. **Grade lift:** B+ → B+

---

## H — Documentation & Onboarding — B+ *(carried forward from 11:00, not re-sampled this run)*

One correction owed: the README's Testing section quotes the frontend coverage figure this run found to be over a subset (D1); update it with the true number when D1 lands.

#### H1 — Add `docs/architecture.md` recording the six load-bearing decisions. **Effort:** S. **Grade lift:** B+ → A−
#### H2 — Three lines at the top of the README pointing at TRACKER, the dogfood reports and this report. **Effort:** S. **Grade lift:** B+ → B+

---

## I — Developer Experience & Tooling — B

Down from B+ on two things this run observed rather than inferred. `bun run verify` — the one gate the README tells everyone to run — **fails on any machine that has run `bun run coverage:frontend` first**, because `eslint.config.js` ignores only `dist` and `src-tauri/target` and the v8 HTML report ships three `.js` files with stale `eslint-disable` directives. That is a defect introduced with this morning's coverage work and it makes the local gate and CI disagree. And `vite.config.ts`'s watcher ignores only `src-tauri/**`, so with the app open under `tauri dev`, every test artefact written into the worktree reloads the live window — observed twice, and tonight that means a wiped chat panel. Everything else holds: CI green in ~5 min, 500-line cap enforced, clippy `-D warnings` clean, the recorder/replay tooling, `cargo llvm-cov` wired up.

#### ~~I1 — Stop ESLint from linting generated output~~ ✓ done 2026-08-28 13:30 (`eslint.config.js` + `scripts/check-loc.mjs` ignore lists)
- **Where:** `draft-assistant/eslint.config.js:8` (`ignores: ["dist", "src-tauri/target"]`)
- **What's wrong:** `coverage/` (and any future `playwright-report/` script) is linted by `eslint .`; three warnings under `--max-warnings=0` fail `verify` after a coverage run. Reproduced this run; CI is green only by ordering.
- **Fix:** `ignores: ["dist", "coverage", "playwright-report", "test-results", "src-tauri/target", "dogfood-output"]`. One line; then `bun run coverage && bun run verify` must pass back-to-back — add that order to CI so it stays true.
- **Effort:** S
- **Grade lift:** B → B+ (the gate stops depending on what you ran before it)

#### ~~I2 — Keep the dev watcher off test and report output~~ ✓ done 2026-08-28 13:30 (`vite.config.ts` watch.ignored; needs a relaunch)
- **Where:** `draft-assistant/vite.config.ts:26-29` (`watch.ignored: ["**/src-tauri/**"]`)
- **What's wrong:** Writes to `coverage/`, `playwright-report/`, `test-results/`, `public/ai-*.json` (the dogfood scripts) and `dogfood-output/` all trigger a full reload of the live window. Tonight's app is a dev instance.
- **Fix:** Add those globs to `watch.ignored`; relaunch once before 17:00 so the running instance has them. Longer term, run draft night on a `tauri build` binary — no watcher, no Vite, and the same code.
- **Effort:** S
- **Grade lift:** B → B+ (the running app stops being a casualty of the test suite)

#### I3 — Add a pre-commit hook running the fast half of `verify`
- **Where:** no `.husky/`, no `lefthook.yml`
- **What's wrong:** Carried. Nothing prevents committing code that fails `tsc` or clippy.
- **Fix:** `lefthook` running `check:loc`, `format:check`, `typecheck`, `lint` — the sub-10-second subset.
- **Effort:** S
- **Grade lift:** B → B+

#### I4 — Split `verify` into fast and full
- **Where:** `package.json` (`verify` is nine steps including a Vite build and a browser run)
- **What's wrong:** Carried. The inner loop pays for a production build and Playwright every time.
- **Fix:** `verify:fast` (LOC, fmt, tsc, unit tests, lint); hook → fast, CI → full.
- **Effort:** S
- **Grade lift:** B → B+
