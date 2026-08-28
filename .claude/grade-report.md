# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-27 (Claude — independent audit, baselined on `.Codex/grade-report.md`)
**Stack:** Tauri 2 desktop app — Rust/Tokio core + React 19/TypeScript-strict/Vite frontend, ~3,700 LOC

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B− | 3 |
| B | Backend Quality | C | 8 |
| C | Frontend Quality | C+ | 6 |
| D | Testing & Reliability | C | 4 |
| E | Security | B | 3 |
| F | Dependencies & Tech Currency | B− | 3 |
| G | Performance & Scalability | B− | 3 |
| H | Documentation & Onboarding | B− | 3 |
| I | Developer Experience & Tooling | C− | 4 |
| **Overall** | | **C+** | **37** |

**Top 5 highest-leverage fixes:** I1, B1, B2, B3, B4

### How this differs from the Codex audit

I first read `.Codex/grade-report.md`, verified every claim in it, and argued its **C+ was too harsh — B−** for a single-user local-first tool. Then I audited independently and found two problems that report missed: **a reachable deadlock** (B1) and **no version control at all** (I1). Those pull it back to **C+ — the same letter as Codex, for entirely different and better-founded reasons.** Their *ranking* is still wrong: their Top 5 leads with test infrastructure while the items that can actually break draft night sit at #3 and #5.

Corrections to that report (detail in its appended addendum): its `glib` RustSec finding is a **false positive** (0 of 13 GTK crates are in the macOS build graph), its two CI items are graded against a team and repo that don't exist, its Vite 8 / TypeScript 7 recommendation is **actively harmful** before a hard deadline, and its kicker finding is right about the symptom but wrong about the mechanism.

### Validation snapshot

- `cargo test`: 13 pass. All 13 live in `draft.rs`, `scoring.rs`, `recommend.rs`, `valuation.rs` — **zero** in `board.rs`, `view.rs`, `engine.rs`, `sleeper.rs`, `lib.rs`.
- `cargo clippy`: clean, zero warnings. `cargo fmt --check`: **fails, 39 files**.
- `npm run build`: passes — 206.63 kB JS / 64.82 kB gzip, 6.47 kB CSS.
- `npm audit`: 0 vulnerabilities / 133 packages. `cargo audit`: 2 advisories + 17 warnings, **all in crates absent from the macOS build**.
- Cold load **measured at 9.9s**; warm 2.6s. The UI claims "~a minute" (`Panels.tsx:21`).
- 210-pick autopilot simulation passes post-mock-draft-changes: 0 duplicate picks, 0 drafted-still-available, all starters filled.
- `git status` → **fatal: not a git repository.**

---

## A — Architecture & Design — B−

The module split is genuinely good: scoring is a data-driven dot product over the league's own `scoring_settings` key space (`scoring.rs:73-82`), which makes exact custom-league scoring nearly free; one `DraftView` serves the UI and the AI export identically. What holds it back is that the dependency graph has **two cycles** and that roster-slot semantics are re-implemented in four places.

#### ~~A1~~ ✓ done 2026-08-27 — Break the two circular module dependencies
- **Where:** `engine.rs:331` ↔ `view.rs:5`; `recommend.rs:4` ↔ `view.rs:7`; type defined at `view.rs:33`
- **What's wrong:** `engine.rs:331` re-exports `build_view`/`merged_picks`/`DraftView` from `view`, while `view.rs:5` imports `LoadedLeague`/`AppConfig`/`now_secs` from `engine` — a cycle. Second cycle: `recommend.rs:4` imports `AvailablePlayer` from `view`, while `view.rs:7` imports `recommend`. Root cause is layering inversion — `AvailablePlayer` is a domain type (a board player plus survival odds) but lives in the top presentation layer, and `engine` re-exports `view`'s API purely as a convenience façade.
- **Fix:** Move `AvailablePlayer` from `view.rs:33` to `board.rs` beside `BoardPlayer`. Delete the `pub use` at `engine.rs:331` and import from `crate::view` at the three call sites (`lib.rs:10`, `lib.rs:26`, `lib.rs:137`, `bin/dump_state.rs:10`). Both cycles disappear.
- **Effort:** S
- **Grade lift:** B− → B (restores a strict one-way dependency graph)

#### ~~A2~~ ✓ done 2026-08-27 — Make roster-slot semantics one shared domain model
- **Where:** `valuation.rs:27-36` and `:80`, `draft.rs:44-94`, `recommend.rs:90`, `board.rs:43-64`, `src/components/Board.tsx:5`
- **What's wrong:** Four independent interpretations of slot eligibility that disagree. `valuation.rs` understands `FLEX`/`WRRB_FLEX`/`REC_FLEX`/`SUPER_FLEX` but then uses only `flex_slots[0]` for *all* flex demand (`valuation.rs:80`, with an in-code comment admitting it). `recommend.rs:90` hardcodes `RB|WR|TE`, so superflex QBs never score as flex-eligible. `Board.tsx:5` omits `K` from the filter list.
- **Fix:** Introduce a `RosterRules`/`SlotKind` type owning eligibility and per-slot demand. Allocate each flex slot type against its own eligible pool. Serialize the league's actual positions into `DraftView` so the frontend builds its filter from state instead of a literal.
- **Effort:** L
- **Grade lift:** B− → B+ (removes the largest source of divergent business rules)

#### ~~A3~~ ✓ done 2026-08-27 — Enforce the Rust↔TypeScript contract that already has a version field
- **Where:** `view.rs:60-61` and `:285-286`, `src/types.ts`, `src/api.ts:24-39` and `:53`
- **What's wrong:** `types.ts` hand-mirrors the Rust structs; `invoke<DraftView>` and the fixture `as DraftView` cast are assertions with no runtime validation. A renamed Rust field compiles cleanly on both sides and fails at render. `schema_version` is emitted at `view.rs:285` and **never read anywhere** — the one field designed to catch this is dead weight.
- **Fix:** Either generate the TS types from the Rust structs, or minimally: check `schema_version` in `api.ts` before accepting a view and throw a legible error on mismatch. The second is 10 lines and closes most of the risk.
- **Effort:** M
- **Grade lift:** B− → B (turns a manual boundary into a checked one)

---

## B — Backend Quality — C

The pure logic is the strongest part of this codebase — `valuation.rs:100-104` constructs its baseline window so `lo < hi` holds for every pool size including 1; `board.rs:70-104` infers bye weeks by *counting* opponent coverage so one stale row can't poison a team; `view.rs:184-197` has a three-tier name fallback so an unknown `player_id` can never break the view. But the concurrency and failure handling around that logic have a reachable deadlock, no timeouts anywhere, and a poller that lies when it fails. Those are draft-night-fatal, and they set the grade.

#### ~~B1~~ ✓ done 2026-08-27 — Fix the lock-order inversion that can permanently freeze the app
- **Where:** `lib.rs:236-237` vs `lib.rs:51/62` and `lib.rs:121/123`
- **What's wrong:** Two opposite lock orders exist. The poll loop takes **`loaded` → `config`**: it holds `loaded` (`lib.rs:236`) and then awaits `config` (`:237`). Two commands take **`config` → `loaded`**: `refresh_data` holds `config` from `:121` across `view_from` and then awaits `loaded` at `:123`; `add_league` holds `config` from `:51` across a synchronous config file write (`:60`) *and* a full `view_from` (`:61`) before awaiting `loaded` at `:62`. That is a textbook cycle, and Tokio's fair mutex does not break it. When it hits, the app wedges permanently — no picks, no recommendations, no recovery but a restart.
- **Impact:** **Critical.** Reachable with one click. "Refresh data" (`App.tsx:105-115`) and re-adding a league are exactly what a user reaches for when the board looks stale — i.e. precisely when the poll loop is also running. `add_league` has the wider window because it holds `config` across a file write.
- **Fix:** Standardize on `loaded` → `config` everywhere. In `refresh_data` and `add_league`, clone the small values needed out of `config` and drop the guard (`drop(config)`) before awaiting `state.loaded.lock()`. Add a comment stating the canonical order.
- **Effort:** S
- **Grade lift:** C → C+ (removes the single most likely way the app dies mid-draft)

#### ~~B2~~ ✓ done 2026-08-27 — Add HTTP timeouts and stop holding a lock across network awaits
- **Where:** `sleeper.rs:174-178`; `lib.rs:101-110`; `lib.rs:213-214`
- **What's wrong:** The client sets only `user_agent` and `gzip` — reqwest applies **no default timeout**, and the only `Duration` in the whole crate is the poll sleep. Two consequences: (a) `refresh_picks` acquires `loaded` at `lib.rs:101` and holds it across **two** HTTP awaits (`:104`, `:106`), so a stalled socket freezes `get_state`, the poll loop, manual picks, undo, and export — indefinitely; (b) the poll loop itself never reaches its sleep, so it stops re-checking the stop/generation flags, meaning live sync dies silently and even `stop_polling` has no effect.
- **Fix:** `.connect_timeout(3s).timeout(8s)` on the client builder. In `refresh_picks`, fetch into locals *before* acquiring `loaded`. Optionally `tokio::join!` the two poll requests.
- **Effort:** S
- **Grade lift:** C → C+ (bounds every failure mode that currently has no bound)

#### ~~B3~~ ✓ done 2026-08-27 — Fall back to stale cache instead of failing the load
- **Where:** `engine.rs:80-87`, `engine.rs:222-224`
- **What's wrong:** `read_cache` returns `None` past TTL (players 24h, projections 6h), and `assemble` then propagates fetch failure with `?`. There is **no stale-cache fallback**. If the undocumented projections endpoint is unavailable once the cache has expired, `load_league` fails outright — no board, no recommendations, no app. That endpoint is undocumented, unversioned, carries the entire valuation model, and has no fallback and no schema-drift detection. With a 6h TTL, an outage at 4 PM against a 9 AM cache produces exactly this at exactly the wrong moment.
- **Fix:** On fetch failure, retry `read_cache` with an effectively infinite TTL, use it, and push a `warnings` entry naming the age. Fail only when there is no cache at all.
- **Effort:** S
- **Grade lift:** C → C+ (removes a single point of failure on the critical path)

#### ~~B4~~ ✓ done 2026-08-27 — Report poll failures instead of swallowing them
- **Where:** `lib.rs:219`, `lib.rs:226`, `lib.rs:240`; `view.rs:87-94`
- **What's wrong:** Both fetches use `if let Ok(...)` with no error arm, and the emit result is discarded with `.ok()`. `DataHealth` has no `last_poll_ok`/`last_poll_at`/`last_error`. So a 500, a rate-limit, or a dead network leaves the board frozen while the UI keeps showing "● Live sync on". During a draft, "nobody has picked in 90 seconds" and "we lost the API 90 seconds ago" demand opposite reactions and are currently indistinguishable.
- **Fix:** Track consecutive failures and last success in `DataHealth`; emit a typed status event on failure and recovery; flip the banner after ~2 consecutive failures. Preserve the last good view — make the staleness unmistakable, don't blank the screen.
- **Effort:** M
- **Grade lift:** C → C+ (a stale board can no longer masquerade as a live one)

#### ~~B5~~ ✓ done 2026-08-27 — Persist manual picks across refresh and restart
- **Where:** `engine.rs:254`, `lib.rs:62`, `lib.rs:123`
- **What's wrong:** `assemble` always sets `manual_picks: Vec::new()`, and both `add_league` and `refresh_data` replace the whole `LoadedLeague`. Clicking "Refresh data" silently erases every manual fallback pick. The failure is self-reinforcing: you only *have* manual picks because the API was lagging, and "Refresh data" is the next thing you'd click.
- **Fix:** Persist manual picks per draft ID via the existing atomic `write_cache` path; reload in `assemble` and reconcile against authoritative API picks, dropping only those the API has caught up on.
- **Effort:** M
- **Grade lift:** C → C+ (closes the documented-but-false fallback promise)

#### B6 — Gate the mock-draft slot fallback on an explicit signal
- **Where:** `view.rs:145-161`, `engine.rs:183`
- **What's wrong:** The mock-draft fallback engages when `user_names.is_empty()`, and `engine.rs:183` builds that map with `.unwrap_or_default()`. So a **transient failure of `/league/{id}/users` in a real league** satisfies the fallback's guard, and the app can adopt the *commissioner's* slot as yours — showing their roster as "my roster" and generating recommendations for their needs, silently, all draft. Requires `my_user_id` to also be unresolved, which is the default first-run state. (Not reachable for the currently configured user, whose ID resolves directly.)
- **Fix:** Carry an explicit `is_mock: bool` on `LoadedLeague`, set only by `load_draft_only`, and gate the fallback on that instead of on an empty map. Add a `warnings` entry whenever the users fetch fails.
- **Effort:** S
- **Grade lift:** C → C+ (removes a silent wrong-team failure introduced with mock support)

#### B7 — Honor `draft_type` and `reversal_round`
- **Where:** `sleeper.rs:65-66` (parsed, zero readers), `draft.rs:8-16`, `sleeper.rs:27-50`
- **What's wrong:** `draft_type` is deserialized and never read; `slot_for_pick` hardcodes snake. For a **linear** draft, `on_clock_slot`, `is_my_pick`, `my_next_picks`, `picks_until_mine`, and the `draft_slot` written by `record_manual_pick` are all wrong. `reversal_round` (3rd-round reversal — an ordinary Sleeper setting) isn't even in `DraftSettings`. Because `build_rosters` uses the API's own `pick.draft_slot`, rosters stay *correct* while the clock goes wrong — a confusing half-broken UI rather than an obviously broken one.
- **Fix:** Short term, warn loudly when `draft_type != "snake"` or `reversal_round` is set. Longer term, implement both in `slot_for_pick` with tests.
- **Effort:** M
- **Grade lift:** C → C+ (stops silently-wrong output in ordinary league configs)

#### ~~B8~~ ✓ done 2026-08-27 — Guard the one unchecked index on the live path
- **Where:** `view.rs:200`
- **What's wrong:** `rosters[(slot - 1) as usize].clone()` — `my_slot` comes from `draft_order` values and is never validated against `settings.teams`. Slot `0` underflows to `u32::MAX` in release (no overflow checks configured) and panics on index; slot > teams panics directly. Notably, `draft.rs:126-129` **already guards exactly this** for pick slots — the check exists, `my_slot` just bypasses it.
- **Fix:** `rosters.get((slot.saturating_sub(1)) as usize).cloned()`, and add a warning when a slot falls outside `1..=teams`.
- **Effort:** S
- **Grade lift:** C → C+ (removes the only reachable panic on the hot path)

---

## C — Frontend Quality — C+

The type discipline is genuinely uncommon: `strict` plus `noUnusedLocals`/`noUnusedParameters`, and **zero** `any`, `as unknown`, non-null assertions, or `@ts-ignore` anywhere in `src/`. Semantic landmarks and a real `<table>` are used correctly, effect dependencies are honest with no suppression comments, and the Tauri unlisten teardown resolves the promise inside cleanup — the thing most codebases get wrong. Against that: there is not a single `aria-*`, `role=`, or `htmlFor` in the entire application source, and the live-sync indicator can lie.

#### ~~C1~~ ✓ done 2026-08-27 — Make the "Live sync on" indicator falsifiable
- **Where:** `App.tsx:144-146`, `App.tsx:15`, `api.ts:20`, `types.ts:109`
- **What's wrong:** `polling` is pure local UI state, set once when the user clicks and never reconciled with reality. There is no error channel from the backend (`api.ts` defines only `draft-updated`), and `generated_at` is emitted by Rust but never rendered. A dead poller looks identical to "nobody has picked yet" — the exact failure this app most needs to surface. Pairs with **B4**; both halves are required.
- **Fix:** Consume the poll-health event from B4; show "last updated Ns ago" from `generated_at`; turn the pill amber/red past a failure threshold.
- **Effort:** M
- **Grade lift:** C+ → B− (the app's most important status signal becomes trustworthy)

#### C2 — Give the confirmation modal real dialog semantics
- **Where:** `App.tsx:181-199`, `components.css:259-283`
- **What's wrong:** Two plain `<div>`s. No `role="dialog"`, `aria-modal`, or accessible name; no focus trap, no initial focus, no Escape handler, no focus restore. Because the modal renders last in the DOM with the background still tabbable, reaching "Confirm" by keyboard means tabbing through up to 200 board rows first. The backdrop click handler is on a non-interactive div with no keyboard equivalent.
- **Fix:** Use native `<dialog>` with `showModal()` — that provides the focus trap, Escape, and background inertness for free. Label it, focus Confirm on open, restore focus to the triggering button on close.
- **Effort:** M
- **Grade lift:** C+ → B− (the only destructive action becomes keyboard-safe)

#### C3 — Add live regions for time-sensitive state
- **Where:** `Panels.tsx:69-105` and `:85`, `App.tsx:201`, `App.tsx:161-163`, `Panels.tsx:57`
- **What's wrong:** "YOU ARE ON THE CLOCK" changes purely from the 3s poll with no user action to prompt a re-read, and has no `aria-live`. The toast is the *only* error channel (7 call sites) and is a bare div with no `role="status"` — so all error feedback is visual-only. Warnings `.join(" · ")` into one unpunctuated run-on.
- **Fix:** `aria-live="assertive"` on the clock banner, `role="status"` on the toast and warnings, `role="alert"` on the setup error with `aria-describedby` to the input. Scope them tightly so the poll doesn't re-announce the page.
- **Effort:** S
- **Grade lift:** C+ → B− (critical live state stops being vision-only)

#### ~~C4~~ ✓ done 2026-08-27 — Complete board control semantics and the empty state
- **Where:** `Board.tsx:32-46`, `Board.tsx:64-94`, `Board.tsx:24`
- **What's wrong:** Position filters convey selection by CSS class only — no `aria-pressed`, no group label. Search relies solely on `placeholder` for its name (the Setup screen does this correctly with real `<label>`s at `Panels.tsx:37-53` — the Board is the inconsistent one). A filter matching nothing renders a header above an empty void. `.slice(0, 200)` truncates with no "showing 200 of N".
- **Fix:** `aria-pressed` + a labeled group on the filters, `aria-label` on search, a single full-width "No matching players" row, and a truncation count.
- **Effort:** S
- **Grade lift:** C+ → B− (completes the primary interaction surface)

#### ~~C5~~ ✓ done 2026-08-27 — Stop the Setup screen flashing on every launch
- **Where:** `App.tsx:14`, `App.tsx:18`, `App.tsx:41-42`
- **What's wrong:** Initial state is `view: null, busy: false`, and `setBusy(true)` only runs *after* `await api.getConfig()` resolves. Every launch with a saved league paints the Setup form first — including firing `autoFocus` on the username input — then swaps to "Loading…". Anything typed in that window is discarded.
- **Fix:** Initialize `busy` to `true`; clear it in the branch that decides no league is configured.
- **Effort:** S
- **Grade lift:** C+ → B− (removes a visible wrong-screen flash on every start)

#### C6 — Order the writes to `view`
- **Where:** `App.tsx:57`, `App.tsx:80/90/108`, `types.ts:109`
- **What's wrong:** `setView` is written by the poll listener and by four awaited handlers with no ordering guard, so a poll landing mid-`await` is overwritten by a stale handler response or vice versa. `generated_at` would identify the newer view and is never read. Related: the open modal reads live `view` while holding a click-time snapshot, so its text mutates under the user and it will still offer "Confirm" for a player live sync has already drafted to someone else.
- **Fix:** Accept a view only if its `generated_at` exceeds the current one. Close the modal if its player leaves `available`.
- **Effort:** S
- **Grade lift:** C+ → B− (removes last-write-wins races on the primary state)

---

## D — Testing & Reliability — C

13 tests pass and they are well-chosen — `draft.rs` checks `picks_for_slot` against the real league document, `recommend.rs` pins the disqualification rules that a simulation caught failing. A genuine 210-pick autopilot simulation with an invariant validator exists and passes. But **every one of the 13 tests covers pure math**: there are zero tests in `board.rs`, `view.rs`, `engine.rs`, `sleeper.rs`, and `lib.rs` — the entire ingestion, assembly, and state-transition surface, which is where all eight B-items live. Not one of B1–B8 would be caught by the current suite.

#### ~~D1~~ ✓ done 2026-08-27 — Wire the existing simulation into `cargo test`
- **Where:** `bin/dump_state.rs:76-106`; create `src-tauri/tests/`
- **What's wrong:** The strongest verification asset in the project is a CLI binary, so it only runs when someone remembers to run it. It is not a regression gate.
- **Fix:** Move the simulation loop and invariant checks into `tests/simulation.rs` against a checked-in fixture (no network). Assert the invariants already validated manually: no duplicate picks, drafted ∉ available, survival ∈ [0,1], roster counts, recommendations non-empty mid-draft.
- **Effort:** M
- **Grade lift:** C → C+ (converts existing work into an automatic gate — best ratio in this category)

#### D2 ◐ partial 2026-08-27 — Add fixture-driven tests for the data path
- **Where:** `board.rs`, `engine.rs`, `sleeper.rs`, `view.rs`; create `src-tauri/tests/fixtures/`
- **What's wrong:** No test covers Sleeper response parsing, cache TTL/refresh behavior, board assembly, bye inference, or `merged_picks`. Upstream payload drift breaks draft night with all 13 tests green.
- **Fix:** Check in sanitized fixtures — standard league, mock draft, partial weekly data, API outage, manual-pick catch-up. Snapshot `DraftView`. Test the cache against a temp dir, including the B3 stale-fallback path.
- **Effort:** L
- **Grade lift:** C → C+ (covers the surface where the real bugs are)
- **Progress:** Added a sanitized Sleeper-shaped board fixture plus cache-expiry, manual-pick reconciliation, full `DraftView`, and 210-pick simulation coverage. Mock-draft and partial-week/mock-HTTP outage fixtures remain.

#### ~~D3~~ ✓ done 2026-08-27 — Add a frontend test runner and cover the live path
- **Where:** no runner configured; `package.json:6-10`
- **What's wrong:** Zero frontend tests and no runner. Filtering, modal actions, poll handling, and error states are entirely unverified.
- **Fix:** Vitest + React Testing Library + user-event, mocking the `api` boundary. Cover setup success/failure, live update handling, filters, manual-pick confirm/undo, and the C1 staleness indicator.
- **Effort:** L
- **Grade lift:** C → C+ (first coverage of the visible product)

#### ~~D4~~ ✓ done 2026-08-27 — Add CI once a repository exists
- **Where:** blocked on **I1**; create `.github/workflows/`
- **What's wrong:** No automation gates build, tests, clippy, fmt, or audit. Genuinely blocked — there is no repo and no remote for CI to attach to.
- **Fix:** After I1, add a macOS workflow running the I2 verify script.
- **Effort:** M
- **Grade lift:** C → C+ (makes local checks continuous) — **do not attempt before I1**

---

## E — Security — B

The attack surface is small and deliberately so, and it holds up under inspection: **every** outbound request is a GET to `api.sleeper.app` (verified by enumerating all URLs in the crate), there are **zero** POST/PUT/PATCH/DELETE calls anywhere, no credentials, no tokens, no telemetry, no third-party endpoints, and the frontend's only `fetch` is a local fixture. npm audit is clean across 133 packages. The remaining gaps are real but modest for a local read-only app.

#### E1 — Define a production CSP
- **Where:** `tauri.conf.json:23`
- **What's wrong:** `"csp": null` removes the primary browser-layer defense in a privileged webview that can invoke every Tauri command.
- **Fix:** Restrict `default-src` to `'self'`, disallow `object`/`embed`, permit only the Vite HMR exception in dev. Verify both dev and packaged builds.
- **Effort:** S
- **Grade lift:** B → B+ (adds the main missing hardening control)

#### E2 — Remove the unused opener plugin
- **Where:** `package.json:16`, `Cargo.toml:17`, `capabilities/default.json:8`, `lib.rs` setup
- **What's wrong:** The plugin is installed, initialized, and granted `opener:default` permission, but grep finds **zero** references in `src/`. Unnecessary privileged surface plus dependency weight.
- **Fix:** Remove the npm dep, the crate, the capability entry, and the `.plugin()` call.
- **Effort:** S
- **Grade lift:** B → B+ (least privilege, smaller graph)

#### E3 — URL-encode the username before interpolating it into a path
- **Where:** `lib.rs:73`, `bin/dump_state.rs:47`
- **What's wrong:** `format!("https://api.sleeper.app/v1/user/{username}")` with no encoding — a username containing `/`, `?`, or `..` alters the request path. Low impact (fixed host, read-only public API, self-supplied input), but it's unvalidated input reaching a URL. Both sites also use bare `reqwest::get`, bypassing the shared client — so they get no gzip, no user agent, and (per B2) no timeout.
- **Fix:** Percent-encode the segment, reject non-alphanumeric usernames, and route both through `SleeperClient`.
- **Effort:** S
- **Grade lift:** B → B+ (closes the only untrusted-input-to-URL path)

---

## F — Dependencies & Tech Currency — B−

Healthier than the Codex report concluded. Both lockfiles are present, Tauri resolves to 2.11.5 and React to 19 within declared ranges, and npm audit is clean. **The `glib` advisories are not a real exposure here** — 13 GTK/glib crates sit in `Cargo.lock`, but `cargo tree --target aarch64-apple-darwin` shows **0** in the macOS build graph (Tauri uses WKWebView on macOS). Remaining items are hygiene, not risk.

#### F1 — Record an audit policy instead of carrying 17 silent warnings
- **Where:** `src-tauri/` — no `audit.toml`
- **What's wrong:** `cargo audit` emits 2 advisories and 17 warnings on every run, all from crates not in the shipped build. Noise at that level is how a *real* advisory gets missed.
- **Fix:** Add `audit.toml` ignoring the Linux-only IDs with a one-line rationale and review date. Keep everything else failing.
- **Effort:** S
- **Grade lift:** B− → B (makes future advisories visible)

#### F2 — Pin the toolchains
- **Where:** no `.nvmrc`, no `rust-toolchain.toml`
- **What's wrong:** Nothing declares the versions that produced the working build (currently rustc 1.88.0, Node 26.7.0, npm 11.19.0).
- **Fix:** Add `rust-toolchain.toml` and `.nvmrc`; note them in the README.
- **Effort:** S
- **Grade lift:** B− → B (reproducible builds)

#### F3 — Defer the frontend major upgrades until after the draft
- **Where:** `package.json:18-24`
- **What's wrong:** Three majors behind — `@vitejs/plugin-react` 4.7.0→6.1.0, Vite 7.3.6→8.2.2, TypeScript 5.8.3→7.0.2. Also two `reqwest` majors (0.12.28 and 0.13.4) resolve simultaneously in `Cargo.lock`.
- **Fix:** **Explicitly deferred.** With no frontend tests (D3) and no version control (I1), these migrations are a net risk increase before a hard deadline. Do Vite + plugin together after the draft, then TypeScript 7 separately.
- **Effort:** M
- **Grade lift:** B− → B (deliberate currency) — **after 2026-08-28**

---

## G — Performance & Scalability — B−

Measured, not assumed: cold load **9.9s**, warm **2.6s**, bundle 65 kB gzipped, ~133 kB per IPC emit. The `changed` gate is the right design — the view is rebuilt ~210 times over three hours rather than 3,600. The Codex report's "first load takes about a minute" premise doesn't hold; the real defect there is that the *UI says* a minute.

#### ~~G1~~ ✓ done 2026-08-27 — Cache the replacement model instead of recomputing it every view
- **Where:** `view.rs:270-282`, `board.rs:178-187`
- **What's wrong:** `build_view` allocates a fresh `Vec<ScoredPlayer>` (~370 String clones) and re-runs `compute_replacement`, re-bucketing and re-sorting every position pool — on **every** emit. It operates on the full board including drafted players, so the result is byte-identical every time and identical to what `build_board` already computed at load. Beyond the waste, it's a correctness hazard: two independent sites computing the baselines means the numbers behind `vorp` and those reported in `replacement_baselines` can silently drift apart if either is edited.
- **Fix:** Store the `ReplacementModel` in `LoadedLeague` at build time; copy it into `DraftView`.
- **Effort:** S
- **Grade lift:** B− → B (removes duplicate work *and* a drift hazard)

#### G2 ◐ partial 2026-08-27 — Fetch the 18 weekly endpoints concurrently, and fix the load message
- **Where:** `engine.rs:152-166`, `src/components/Panels.tsx:21`
- **What's wrong:** 18 requests run strictly sequentially; one slow week delays all the rest. Measured at 9.9s total — real but far from the "~a minute" the UI promises, which is its own defect: the app overstates its own slowness by 6×.
- **Fix:** Bounded concurrency (semaphore of ~6), preserving per-week degradation warnings; sort deterministically after collection. Correct the message to "~10 seconds".
- **Effort:** M
- **Grade lift:** B− → B (cuts load to ~2s and stops misinforming the user)
- **Progress:** The misleading “~a minute” copy now says “~10 seconds.” Bounded-concurrency fetching remains intentionally deferred from the pre-draft batch.

#### G3 — Precompute tier counts and stop cloning the whole board per emit
- **Where:** `recommend.rs:156-159`, `view.rs:203-212`
- **What's wrong:** `tier_left` rescans all of `available` for every candidate inside both mode loops — ≈100 × 370 × 2 ≈ 74k string comparisons. `build_view` clones ~370 `BoardPlayer` structs (4-5 heap strings each) into `available`, ~2,000 allocations per emit. Sub-millisecond in absolute terms; listed for accuracy, not urgency.
- **Fix:** Build a `HashMap<(&str, u32), usize>` of tier counts once before the loops. Consider borrowing rather than cloning for `available`.
- **Effort:** S
- **Grade lift:** B− → B (removes the only super-linear work on the live path)

---

## H — Documentation & Onboarding — B−

The README is well above typical for this stage — it explains the product, the data sources including the undocumented endpoint, cache locations and TTLs, the file layout, and the browser fixture mode. It loses ground on accuracy: three claims don't match the code, and there's no troubleshooting or verification guidance.

#### H1 — Document the supported roster/scoring matrix and the known gaps
- **Where:** `README.md:12-35`
- **What's wrong:** It claims exact adherence to league settings without disclosing that kickers are never fetched (`sleeper.rs:230,242`) so a K slot can never be filled, that mixed-flex and superflex are valued off `flex_slots[0]` only, that linear and 3RR drafts compute the clock wrongly (B7), and that manual picks don't survive a refresh (B5).
- **Fix:** Add a support matrix — supported / degraded / unsupported — and tighten the claims until B5/B7 and A2 land.
- **Effort:** S
- **Grade lift:** B− → B (expectations match behavior)

#### H2 — Fix the multi-league claim
- **Where:** `README.md:34-35`
- **What's wrong:** "Multi-league. Leagues are stored in config; switching is a config value" — but there is no league list or switcher in the UI (grep finds zero frontend references to `leagues`) and no `set_active_league` command. The only way to switch is to re-enter a league ID, which also triggers a full reload.
- **Fix:** Either add a switcher backed by the stored list, or restate it as "re-enter a league ID to switch."
- **Effort:** S
- **Grade lift:** B− → B (removes a feature claim with no implementation)

#### H3 — Add troubleshooting and a verification section
- **Where:** `README.md:37-75`
- **What's wrong:** No canonical "is this change ready?" command, and no recovery guidance for stale projections, corrupted cache, partial weekly data, or a live-sync outage near draft time.
- **Fix:** Document the I2 verify script and add a symptom → signal → action table, including how to reset local state and what is lost.
- **Effort:** S
- **Grade lift:** B− → B (external failures become recoverable)

---

## I — Developer Experience & Tooling — C−

The declared foundations are good — TypeScript strict with unused-locals/params, clippy clean under `-D warnings`, a 343ms production build, a browser fixture mode, and a headless `dump_state` CLI. One fact dominates all of it: **there is no version control.**

#### I1 — Put this project under version control
- **Where:** project root — `git status` → `fatal: not a git repository`
- **What's wrong:** ~3,700 lines of hand-written source on a **removable flash volume** (`/Volumes/512Flash`), with no history, no branches, no backup, and no way to revert a bad edit. Every fix in this report — several touching the live draft path, several rated L — would be made with no undo, the day before the draft. A `.gitignore` already exists at `draft-assistant/.gitignore`; nothing else is needed. The Codex audit graded the absence of *CI* as Major twice while never noticing there is nothing for CI to run against.
- **Impact:** **Critical**, and it blocks D4 and gates the safety of everything else.
- **Fix:**
  ```bash
  git init && git add -A && git commit -m "Working draft assistant before hardening"
  ```
  Then consider a remote or a copy off the flash volume.
- **Effort:** S (about 30 seconds)
- **Grade lift:** C− → C+ (makes every other change reversible)

#### ~~I2~~ ✓ done 2026-08-27 — Add one all-up verification command
- **Where:** `package.json:6-10`
- **What's wrong:** Only `dev`/`build`/`preview`/`tauri` exist. Verification means remembering four commands across two toolchains, so checks get skipped — `cargo fmt` already has.
- **Fix:** Add `typecheck`, `test`, `test:rust`, `lint`, and a `verify` that chains fmt check → tsc → frontend build → cargo test → clippy, failing loudly.
- **Effort:** S
- **Grade lift:** C− → C (one repeatable gate)

#### ~~I3~~ ✓ done 2026-08-27 — Enforce formatting and add a frontend linter
- **Where:** 39 files fail `cargo fmt --check`; no ESLint/Prettier config
- **What's wrong:** Formatting drift across most of the Rust source obscures real diffs, and React/TS has no semantic linter — no exhaustive-deps rule, no hooks rules. The effect dependencies are currently correct by hand, with nothing enforcing that.
- **Fix:** Run `cargo fmt --all` as one isolated commit (after I1), then add ESLint with `react-hooks` and TypeScript rules, exposed via I2's scripts.
- **Effort:** M
- **Grade lift:** C− → C (consistency stops being manual)

#### ~~I4~~ ✓ done 2026-08-27 — Configure a release profile with overflow checks on the live path
- **Where:** `Cargo.toml` — no `[profile.release]`
- **What's wrong:** Release builds run with `overflow-checks = false`, so the underflows noted in B8 and `view.rs:119-122` wrap silently into garbage numbers rather than failing loudly. For a tool whose entire output is derived arithmetic, silent wrong numbers are worse than a crash.
- **Fix:** Add `[profile.release]` with `overflow-checks = true` (negligible cost at this workload), plus `lto = "thin"` and `strip = true` to offset the 12 MB binary.
- **Effort:** S
- **Grade lift:** C− → C (arithmetic corruption becomes detectable)

---

## Draft-night triage — the only ordering that matters before 2026-08-28 17:00 PDT

| Order | Item | Why now | Effort |
|-------|------|---------|--------|
| 1 | **I1** | 30 seconds, and every other change becomes reversible | S |
| 2 | **B1** | One button click can permanently freeze the app mid-draft | S |
| 3 | **B2** | A stalled socket has no bound and wedges the poll loop and UI | S |
| 4 | **B3** | Stale cache + endpoint outage = no board at all | S |
| 5 | **B4 + C1** | A dead poller currently looks identical to a quiet draft | M |
| 6 | **B5** | "Refresh data" erases exactly the fallback you needed it for | M |

Everything else in this report — accessibility, CSP, the flex/superflex model, tests, CI, formatting, toolchain currency — is genuine and worth doing, and none of it belongs before the draft.

The three simulation-discovered recommendation bugs fixed earlier (positional discipline, VORP normalization, per-position candidate pool) remain fixed; the 210-pick autopilot still produces a valid roster with all starters filled and every invariant passing after today's mock-draft changes.
