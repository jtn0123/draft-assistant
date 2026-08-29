# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-27
**Stack:** Tauri 2 desktop app with a Rust/Tokio core and React 19/TypeScript/Vite frontend

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B− | 3 |
| B | Backend Quality | C+ | 4 |
| C | Frontend Quality | B− | 3 |
| D | Testing & Reliability | C− | 4 |
| E | Security | B− | 2 |
| F | Dependencies & Tech Currency | C+ | 3 |
| G | Performance & Scalability | B− | 3 |
| H | Documentation & Onboarding | B | 3 |
| I | Developer Experience & Tooling | C+ | 4 |
| **Overall** | | **C+** | **29** |

**Top 5 highest-leverage fixes:** B1, D1, B2, D2, E1

> **⚠️ Verification addendum added 2026-08-27 by Claude.** Every claim in this
> report was independently checked against the code. Most are confirmed, but
> **three findings are false positives** and **one is wrong about its own
> mechanism**, and the report **misses the project's two largest risks**.
> See [Verification addendum](#verification-addendum--claude-2026-08-27) at the
> end of this file before acting on the "Top 5" line above — that ranking is
> not the right order of work.

### Validation snapshot

- `npm run build`: passed; production bundle 206.63 kB JavaScript / 64.82 kB gzip and 6.47 kB CSS / 1.88 kB gzip.
- `cargo test --all-targets`: passed, 13 tests; the app and CLI binary targets contain no direct tests.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `npm audit --json`: zero known vulnerabilities across 133 packages.
- `cargo audit`: no vulnerability failures, but 17 allowed warnings, including one unsound `glib` advisory and 16 unmaintained transitive crates.
- `cargo fmt --all -- --check`: failed across multiple Rust files.
- No repository-owned frontend tests, integration tests, coverage configuration, or CI workflows were found.

---

## A — Architecture & Design — B−

The Rust core has clear, compact modules for scoring, valuation, draft state, recommendations, API access, and view construction (`draft-assistant/src-tauri/src/scoring.rs`, `valuation.rs`, `draft.rs`, `recommend.rs`, `sleeper.rs`, and `view.rs`). A single serialized `DraftView` also gives the UI and export path one authoritative state shape (`src-tauri/src/view.rs:58-94`). The main architectural weakness is that roster eligibility and the Rust-to-TypeScript contract are repeated rather than generated or centralized, which already creates divergent behavior.

#### ~~A1~~ ✓ done 2026-08-27 — Make roster-slot semantics one shared domain model
- **Where:** `draft-assistant/src-tauri/src/valuation.rs:27-36`, `draft-assistant/src-tauri/src/draft.rs:44-94`, `draft-assistant/src-tauri/src/recommend.rs:33-100`, `draft-assistant/src/components/Board.tsx:5`
- **What's wrong:** Slot eligibility is interpreted independently by valuation, roster filling, recommendation scoring, and the UI. The implementations disagree: valuation understands several flex types, recommendations only understand `FLEX`, and the UI filter omits `K`.
- **Impact:** Major — a supported Sleeper roster can receive internally inconsistent replacement levels, needs, recommendations, and controls.
- **Fix:** Introduce a Rust `RosterRules`/`SlotKind` domain model that owns eligibility, scarcity rules, and displayable positions. Pass it to valuation, roster filling, and recommendations; serialize supported positions into `DraftView` so the frontend builds filters from state rather than a hard-coded list.
- **Effort:** L
- **Grade lift:** B− → B+ (removes the largest cross-module source of divergent business rules)

#### ~~A2~~ ✓ done 2026-08-27 — Generate or validate the Rust-to-TypeScript state contract
- **Where:** `draft-assistant/src-tauri/src/view.rs:13-94`, `draft-assistant/src-tauri/src/recommend.rs:8-22`, `draft-assistant/src/types.ts:1-134`, `draft-assistant/src/api.ts:49-55`
- **What's wrong:** The frontend manually mirrors Rust structs, and browser fixture JSON is trusted with a TypeScript cast rather than validated. A Rust field change can compile successfully while producing a runtime-only frontend break.
- **Impact:** Moderate — schema drift can break the draft-night UI despite both language toolchains passing independently.
- **Fix:** Generate TypeScript types from the serialized Rust API using a Tauri-compatible binding tool, or emit JSON Schema and validate the fixture/API boundary in development and tests. Add a schema-version compatibility check before accepting `DraftView`.
- **Effort:** M
- **Grade lift:** B− → B (turns a fragile manual boundary into an enforced contract)

#### A3 — Separate polling lifecycle from command registration
- **Where:** `draft-assistant/src-tauri/src/lib.rs:17-23`, `draft-assistant/src-tauri/src/lib.rs:181-254`, `draft-assistant/src-tauri/src/lib.rs:256-291`
- **What's wrong:** `lib.rs` owns state storage, eleven commands, poll-task lifecycle, event emission, and application bootstrapping. It is still under the project's size limit, but unrelated responsibilities make concurrency and lifecycle behavior harder to test in isolation.
- **Impact:** Moderate — changes to commands and live synchronization share locks and state machinery, increasing regression risk around the app's most time-sensitive feature.
- **Fix:** Extract a `PollController` with start/stop/status/error events and move Tauri command adapters into a `commands` module. Keep `lib.rs` limited to composition and handler registration.
- **Effort:** M
- **Grade lift:** B− → B (gives live-sync behavior a testable boundary without changing product behavior)

---

## B — Backend Quality — C+

The core is small, type-safe, deterministic, and generally returns errors instead of panicking (`src-tauri/src/lib.rs:43-179`); scoring and recommendation logic are intentionally auditable. However, two current behaviors violate important product promises: fallback picks are volatile, and recommendations are not actually generic across Sleeper roster shapes. Network and persistence failures are also frequently discarded rather than represented in `DataHealth`.

#### ~~B1~~ ✓ done 2026-08-27 — Preserve manual picks across refreshes and reloads
- **Where:** `draft-assistant/src-tauri/src/lib.rs:113-124`, `draft-assistant/src-tauri/src/lib.rs:127-165`, `draft-assistant/src-tauri/src/engine.rs:247-260`
- **What's wrong:** Manual picks live only inside `LoadedLeague`. Every full `refresh_data`, league reload, or application restart constructs a new `LoadedLeague` with `manual_picks: Vec::new()`, silently erasing the fallback state the README promises.
- **Impact:** Major — using Refresh Data during an API outage can put drafted players back on the board and corrupt recommendations during a live draft.
- **Fix:** Persist manual picks per draft ID using an atomic cache file; reload and reconcile them against authoritative API picks in `assemble`; clear only overlapping picks as the API catches up. Add regression tests for refresh, restart, partial catch-up, and undo.
- **Effort:** M
- **Grade lift:** C+ → B− (closes the highest-risk draft-night state-loss path)

#### ~~B2~~ ✓ done 2026-08-27 — Correct recommendations for superflex, mixed flex, and kicker leagues
- **Where:** `draft-assistant/src-tauri/src/recommend.rs:33-45`, `draft-assistant/src-tauri/src/recommend.rs:88-153`, `draft-assistant/src-tauri/src/valuation.rs:70-91`, `draft-assistant/src-tauri/src/board.rs:43-64`
- **What's wrong:** Recommendations only treat RB/WR/TE as flex-eligible and apply RB/WR depth heuristics to every unrecognized position, including kickers. Valuation explicitly uses only the first flex slot's eligibility for all flex demand, so a league mixing `FLEX`, `REC_FLEX`, or `SUPER_FLEX` is valued incorrectly.
- **Impact:** Major — users in common nonstandard leagues can receive strategically wrong picks even though the app claims to follow the exact roster and scoring settings.
- **Fix:** Allocate each flex slot type against its own eligible pool, make need scoring consult shared slot eligibility, and add explicit QB/TE/DEF/K policies based on actual starting demand. Cover standard, superflex, mixed-flex, and kicker fixtures.
- **Effort:** L
- **Grade lift:** C+ → B (makes the core recommendation claim true across supported Sleeper leagues)

#### ~~B3~~ ✓ done 2026-08-27 — Report live-sync failures instead of swallowing them
- **Where:** `draft-assistant/src-tauri/src/lib.rs:212-241`, `draft-assistant/src/App.tsx:55-75`, `draft-assistant/src/types.ts:99-105`
- **What's wrong:** The poller ignores failed pick/draft calls and ignores event-emission errors. The frontend continues displaying “Live sync on,” and `DataHealth` has no last-success, last-error, or consecutive-failure fields.
- **Impact:** Major — a stale board can look live during the time-sensitive part of a draft.
- **Fix:** Track poll health in state, emit a typed status event on failures and recovery, expose last successful sync time, and change the live control/banner after a short failure threshold. Preserve the last valid view while making staleness unmistakable.
- **Effort:** M
- **Grade lift:** C+ → B− (prevents silent stale-state failures)

#### B4 — Propagate persistence failures
- **Where:** `draft-assistant/src-tauri/src/engine.rs:70-111`, `draft-assistant/src-tauri/src/lib.rs:50-63`, `draft-assistant/src-tauri/src/lib.rs:68-80`
- **What's wrong:** Data-directory creation, cache writes/renames, and config writes discard all I/O errors. Commands report success even when the league, username, or fetched data was not saved.
- **Impact:** Moderate — users may believe setup or refresh succeeded and then lose state after restart, with no actionable explanation.
- **Fix:** Return `Result` from `Engine::new`, `write_cache`, and `save_config`; distinguish cache-write warnings from fatal config-write failures; surface failures through typed command errors and `DataHealth`.
- **Effort:** M
- **Grade lift:** C+ → B− (makes local-first persistence observable and dependable)

---

## C — Frontend Quality — B−

The runtime layout is visually coherent and information-dense, with useful loading, warning, empty-roster, and confirmation states (`src/App.tsx:117-201`, `src/components/Panels.tsx:137-199`). Component boundaries are reasonable for a 60 KB source tree. Accessibility and failure-state semantics lag behind the visual quality: the custom modal, transient messages, tabs, and search control are not expressed to assistive technology.

#### C1 — Make the confirmation modal keyboard and screen-reader safe
- **Where:** `draft-assistant/src/App.tsx:181-199`, `draft-assistant/src/components.css:259-283`
- **What's wrong:** The modal is a generic `div` without dialog semantics, an accessible name, initial focus, focus trapping, Escape handling, or focus restoration. Keyboard focus can remain on and activate controls behind the backdrop.
- **Impact:** Major — keyboard and assistive-technology users cannot reliably or safely confirm a manual draft action.
- **Fix:** Use the native `<dialog>` element or an accessible dialog primitive; label it, focus Confirm or Cancel on open, trap focus, close on Escape, restore focus to the initiating Draft button, and test the complete keyboard flow.
- **Effort:** M
- **Grade lift:** B− → B (fixes the most consequential interaction accessibility gap)

#### C2 — Announce live draft, warning, error, and toast changes
- **Where:** `draft-assistant/src/components/Panels.tsx:64-106`, `draft-assistant/src/App.tsx:161-163`, `draft-assistant/src/App.tsx:201`, `draft-assistant/src/components/Panels.tsx:57`
- **What's wrong:** Time-sensitive clock changes, data warnings, setup errors, and transient toasts have no `aria-live`/status/alert semantics. Visual users see updates, while screen-reader users may receive no notification.
- **Impact:** Major — the app's defining live “on the clock” and stale-data signals can be missed.
- **Fix:** Add appropriately scoped `role="status"` and `role="alert"` regions, avoid re-announcing the entire page on every poll, and add component tests for urgent versus polite announcements.
- **Effort:** S
- **Grade lift:** B− → B (makes critical live state available beyond vision)

#### ~~C3~~ ✓ done 2026-08-27 — Give board controls complete accessible semantics
- **Where:** `draft-assistant/src/components/Board.tsx:27-47`, `draft-assistant/src/components/Board.tsx:48-95`
- **What's wrong:** The position selector is visually tab-like but has neither pressed-state nor tab semantics, and player search relies on placeholder text without a label. The empty filtered state renders a blank table body with no explanation.
- **Impact:** Moderate — board navigation is ambiguous for assistive technology and gives weak feedback when a filter has no matches.
- **Fix:** Use a labeled toolbar with `aria-pressed` filter buttons, associate a visible or screen-reader label with search, and render a single full-width “No matching players” row when empty.
- **Effort:** S
- **Grade lift:** B− → B (completes the primary board interaction pattern)

---

## D — Testing & Reliability — C−

The 13 Rust unit tests exercise useful math and recommendation guardrails and all pass (`src-tauri/src/scoring.rs:108-159`, `draft.rs:146-201`, `recommend.rs:238-333`, `valuation.rs:139-173`). But the data ingestion, view assembly, Tauri command/state lifecycle, frontend, and actual desktop journey are untested. There is no CI or coverage gate, so today's clean local result is not continuously enforced.

#### D1 ◐ partial 2026-08-27 — Add fixture-driven backend integration tests `[BE]`
- **Where:** Broad gap across `draft-assistant/src-tauri/src/board.rs`, `engine.rs`, `sleeper.rs`, `view.rs`, and `lib.rs`; create `draft-assistant/src-tauri/tests/`
- **What's wrong:** None of the Sleeper response parsing, cache lifecycle, board assembly, merged picks, full `DraftView`, or polling state is tested end to end. Current tests construct small in-memory values and miss contract and state-transition failures.
- **Impact:** Major — upstream payload drift or a refresh/poller regression can break draft-night behavior while all 13 tests remain green.
- **Fix:** Add checked-in sanitized Sleeper fixtures for standard, superflex, partial weekly data, API outage, and manual-pick catch-up scenarios. Exercise assembly through `DraftView` snapshots and test cache refresh/reload behavior with temporary directories and a mock HTTP server.
- **Effort:** L
- **Grade lift:** C− → C+ (covers the app's core data and state boundaries)
- **Progress:** Added a checked-in, network-free 210-pick integration simulation that repeatedly builds `DraftView` and gates uniqueness, availability, probability, roster, recommendation, and starter-completion invariants. Sleeper parsing and mock-HTTP cache scenarios remain.

#### D2 ◐ partial 2026-08-27 — Add frontend component and interaction tests `[FE]`
- **Where:** Broad gap across `draft-assistant/src/App.tsx`, `src/components/Board.tsx`, and `src/components/Panels.tsx`; create `draft-assistant/src/**/*.test.tsx`
- **What's wrong:** There is no frontend test runner or repository-owned frontend test. Filtering, setup failures, polling state, modal actions, live events, warning states, and accessibility behavior are unchecked.
- **Impact:** Major — user-facing regressions can ship even when TypeScript compiles and Rust tests pass.
- **Fix:** Add Vitest, React Testing Library, and user-event; mock the API boundary; cover setup success/failure, live updates, filters, manual-pick confirmation/undo, refresh failures, empty states, and accessible dialog/status behavior.
- **Effort:** L
- **Grade lift:** C− → C+ (adds meaningful coverage of the entire visible product surface)
- **Progress:** Added Vitest, Testing Library, user-event, and seven tests covering setup/no-saved-league and error paths, live updates and staleness, manual pick/undo, dynamic filters, empty search, and schema compatibility. Refresh/export failures and accessible dialog/status behavior remain.

#### D3 — Add a real desktop smoke journey `[both]`
- **Where:** Broad gap; create `draft-assistant/e2e/` and drive the Tauri app or a testable browser adapter against `public/dev-fixture.json`
- **What's wrong:** No test proves that Rust commands, event payloads, the webview, and the rendered UI work together. The browser fallback is a cast fixture with no parity assertion against the desktop command boundary.
- **Impact:** Major — packaging or IPC failures are invisible to unit tests and can make the built app unusable.
- **Fix:** Add one deterministic smoke test that launches the packaged/debug app, loads a fixture-backed league, verifies recommendations and on-clock state, records and undoes a manual pick, and exports state. Run it at least on macOS release candidates.
- **Effort:** L
- **Grade lift:** C− → C+ (establishes physical confidence in the shipped desktop journey)

#### D4 ◐ partial 2026-08-27 — Enforce tests and coverage in CI `[both]`
- **Where:** No `.github/workflows/` exists; `draft-assistant/package.json:6-10` has no test script and `draft-assistant/src-tauri/Cargo.toml:1-22` has no coverage tooling
- **What's wrong:** Builds, tests, Clippy, formatting, audits, and coverage are not automatically gated. There is no baseline to reveal untested growth.
- **Impact:** Moderate — quality depends on remembering a set of local commands, and regressions can enter unnoticed.
- **Fix:** Add a CI workflow for frontend build/tests, Rust fmt/Clippy/tests, dependency audits, and artifact packaging. Publish frontend and Rust coverage, set an honest initial threshold based on measured coverage, then raise it with meaningful tests.
- **Effort:** M
- **Grade lift:** C− → C+ (turns local checks into a repeatable reliability contract)
- **Progress:** Added a macOS GitHub Actions workflow running the unified format, typecheck, build, Rust test, ESLint, and Clippy gate. Coverage publication, audits, and packaging remain.

---

## E — Security — B−

The attack surface is relatively small: the app is local-first, calls fixed HTTPS Sleeper origins, uses no secrets or authentication tokens, and npm audit is clean. Tauri capability permissions are short (`src-tauri/capabilities/default.json:1-10`). The webview nevertheless has no content security policy and includes an opener plugin/permission that the frontend does not use.

#### E1 — Define a restrictive Tauri content security policy
- **Where:** `draft-assistant/src-tauri/tauri.conf.json:12-24`
- **What's wrong:** `csp` is explicitly `null`, removing a key defense against script/style injection in the privileged desktop webview.
- **Impact:** Major — a future rendering or dependency flaw would have fewer barriers before it could invoke exposed Tauri commands.
- **Fix:** Add a production CSP limited to local assets, disallow remote scripts and object/embed content, and permit only the minimum development exceptions needed for Vite HMR. Verify both dev and packaged builds.
- **Effort:** S
- **Grade lift:** B− → B+ (adds the primary browser-layer hardening control for Tauri)

#### E2 — Remove the unused opener capability and dependency
- **Where:** `draft-assistant/package.json:12-17`, `draft-assistant/src-tauri/Cargo.toml:16-22`, `draft-assistant/src-tauri/capabilities/default.json:5-9`, `draft-assistant/src-tauri/src/lib.rs:256-260`
- **What's wrong:** The opener plugin is installed, initialized, and granted default permission, but no frontend source imports or invokes it. This is unnecessary privileged surface and dependency weight.
- **Impact:** Moderate — unused native capabilities expand what compromised webview code could attempt and add transitive maintenance exposure.
- **Fix:** Remove `@tauri-apps/plugin-opener`, `tauri-plugin-opener`, its capability permission, and initialization. Re-add only a narrowly scoped URL rule if the product later needs external links.
- **Effort:** S
- **Grade lift:** B− → B (applies least privilege and simplifies the dependency graph)

---

## F — Dependencies & Tech Currency — C+

Both npm and Cargo lockfiles are present, the app resolves to current Tauri 2 and React 19 releases within its declared ranges, and npm audit reports no vulnerabilities. Live `cargo audit` found no blocking vulnerability but did find one unsound `glib` advisory plus 16 unmaintained transitive crates; most GTK warnings are target-specific but matter for the configured all-platform bundle and future portability. The frontend also sits one or more major releases behind the latest Vite/plugin/TypeScript toolchain.

#### F1 — Resolve or formally scope the RustSec warning set
- **Where:** `draft-assistant/src-tauri/Cargo.lock` via the Tauri dependency graph; direct entry points are `draft-assistant/src-tauri/Cargo.toml:13-22`
- **What's wrong:** `cargo audit` reports `RUSTSEC-2024-0429` (`glib` unsoundness) and 16 unmaintained-crate warnings, including GTK3 bindings and `proc-macro-error`/`unic-*`. The project has no checked-in audit policy explaining target-specific exceptions.
- **Impact:** Major — an unsound transitive crate is a real supply-chain warning, while unmanaged exceptions make future actionable advisories easy to miss.
- **Fix:** Update Tauri and transitive dependencies where fixes exist, confirm which crates are excluded from macOS artifacts with target-specific inspection, and add a minimal deny/ignore policy containing advisory IDs, target rationale, owner, and review date. Keep new vulnerabilities failing CI.
- **Effort:** M
- **Grade lift:** C+ → B (turns an unmanaged warning set into a reviewed dependency posture)

#### F2 — Plan and verify frontend toolchain major upgrades
- **Where:** `draft-assistant/package.json:18-24`
- **What's wrong:** Live registry checks show `@vitejs/plugin-react` 4.7.0 versus 6.1.0 latest, Vite 7.3.6 versus 8.2.2, and TypeScript 5.8.3 versus 7.0.2. These are not current vulnerabilities, but multiple major gaps increase future migration cost.
- **Impact:** Moderate — delaying coordinated toolchain upgrades compounds compatibility work and leaves performance/tooling improvements unused.
- **Fix:** Upgrade Vite and its React plugin together on a dedicated change, then assess TypeScript 7 separately. Run the production build, browser preview, packaged Tauri build, and frontend tests before accepting each major jump.
- **Effort:** M
- **Grade lift:** C+ → B− (restores a deliberate, current frontend baseline)

#### F3 — Automate dependency-update visibility
- **Where:** No dependency bot or update workflow exists; manifests are `draft-assistant/package.json` and `draft-assistant/src-tauri/Cargo.toml`
- **What's wrong:** Dependency freshness and advisories are only visible when someone manually runs npm and Cargo checks.
- **Impact:** Moderate — security and compatibility drift can accumulate silently between audits.
- **Fix:** Configure grouped, low-noise update PRs for npm and Cargo, run build/test/audit gates on them, and document how target-specific RustSec warnings are reviewed.
- **Effort:** S
- **Grade lift:** C+ → B− (makes dependency health continuously visible)

---

## G — Performance & Scalability — B−

The frontend bundle is lean, board filtering is memoized and capped to 200 displayed rows (`src/components/Board.tsx:19-25`), and the app caches large player/projection payloads. The first-load path is intentionally slow because it serially downloads 18 weekly endpoints, and HTTP requests have no explicit timeout. View rebuilding also recomputes replacement levels that were already established while building the board.

#### G1 — Fetch weekly projections with bounded concurrency
- **Where:** `draft-assistant/src-tauri/src/engine.rs:141-169`, `draft-assistant/src/components/Panels.tsx:21`
- **What's wrong:** Eighteen independent weekly requests run one after another, which is why the UI warns that first load can take about a minute. One slow request delays every subsequent week.
- **Impact:** Major — onboarding and forced refresh are much slower than necessary and especially fragile near draft time.
- **Fix:** Fetch weeks concurrently with a small semaphore limit, preserve per-week degradation warnings, and sort/annotate results deterministically after collection. Add a timing-focused test with delayed mock responses.
- **Effort:** M
- **Grade lift:** B− → B+ (removes the dominant avoidable startup latency)

#### G2 — Add explicit HTTP timeouts and retry policy
- **Where:** `draft-assistant/src-tauri/src/sleeper.rs:172-195`
- **What's wrong:** The shared `reqwest::Client` has no connect or total request timeout and no bounded retry/backoff policy. A stalled endpoint can leave setup or refresh busy indefinitely.
- **Impact:** Major — an unreliable network can freeze the app during its most important workflow.
- **Fix:** Configure short connect and bounded request timeouts, retry only idempotent transient failures with jitter, and return a typed error that distinguishes timeout, HTTP status, and invalid payload.
- **Effort:** S
- **Grade lift:** B− → B (bounds failure time and improves recovery behavior)

#### ~~G3~~ ✓ done 2026-08-27 — Stop recomputing replacement levels for every view
- **Where:** `draft-assistant/src-tauri/src/board.rs:175-190`, `draft-assistant/src-tauri/src/view.rs:269-282`
- **What's wrong:** `build_board` computes replacement demand/baselines to assign VORP, but `build_view` reconstructs the scored vector and computes the same model again. Every pick update rebuilds a full view even though league settings and the board did not change.
- **Impact:** Minor — current datasets are manageable, but repeated duplicate work consumes CPU on every live state emission.
- **Fix:** Store `ReplacementModel` or its serializable demand/baseline outputs in `LoadedLeague` when the board is built and copy them into `DraftView`.
- **Effort:** S
- **Grade lift:** B− → B (removes unnecessary repeated calculation from the live path)

---

## H — Documentation & Onboarding — B

The README clearly explains the product, architecture, run/build commands, fixture preview, cache location, data sources, and file layout (`README.md:1-94`). It is substantially better than typical early-stage documentation. It does not tell contributors how to run the validation suite, recover from known data/API failures, or understand which roster variants are not yet exact.

#### H1 — Document the supported roster/scoring matrix and limitations
- **Where:** `draft-assistant/README.md:12-35`, especially the exact-scoring and multi-league claims
- **What's wrong:** The README presents generic exact behavior without disclosing current mixed-flex, superflex recommendation, kicker, undocumented projection, or manual-pick persistence limitations.
- **Impact:** Major — users can reasonably trust recommendations in league shapes the implementation does not model consistently.
- **Fix:** Add a support matrix for slot types, scoring features, draft types, and known degradation modes. Tighten claims until B2 and B1 are fixed, then link each scenario to a fixture/integration test.
- **Effort:** S
- **Grade lift:** B → B+ (aligns product expectations with current behavior)

#### H2 — Add a contributor verification section
- **Where:** `draft-assistant/README.md:37-65`, `draft-assistant/package.json:6-10`
- **What's wrong:** Setup and build are documented, but tests, Clippy, formatting, audits, browser fixture validation, and release smoke checks are absent.
- **Impact:** Moderate — contributors cannot readily reproduce the quality checks used for this audit.
- **Fix:** Document one canonical local verification command plus the underlying frontend and Rust commands, required tool versions, expected fixture behavior, and the distinction between browser preview and desktop parity.
- **Effort:** S
- **Grade lift:** B → B+ (makes onboarding lead to a verified change, not just a running app)

#### H3 — Add operational troubleshooting and cache recovery
- **Where:** `draft-assistant/README.md:67-75`, `draft-assistant/src-tauri/src/engine.rs:80-111`
- **What's wrong:** Cache location and TTLs are listed, but there is no guide for stale projections, corrupted JSON, partial weekly data, username/league changes, live-sync outages, or safely resetting local state.
- **Impact:** Moderate — users have no clear recovery path when the external API or local cache misbehaves near draft time.
- **Fix:** Add a concise troubleshooting table with symptoms, data-health signals, safe refresh/reset steps, what state is lost, and where exported state can aid diagnosis.
- **Effort:** S
- **Grade lift:** B → B+ (turns external-data failures into recoverable operations)

---

## I — Developer Experience & Tooling — C+

TypeScript strictness, unused-code checks, lockfiles, a fast production build, passing strict Clippy, and a browser fixture provide a good local foundation (`tsconfig.json:3-24`, `package.json:6-24`, `src/api.ts:42-90`). The project lacks a single verification entry point, lint/test tooling for React, CI, version pins, and formatting enforcement; `cargo fmt --check` currently fails.

#### ~~I1~~ ✓ done 2026-08-27 — Add one all-up verification command
- **Where:** `draft-assistant/package.json:6-10`
- **What's wrong:** Developers must know separate npm and Cargo commands, and the existing scripts expose only dev/build/preview/Tauri. There is no canonical answer to “is this change ready?”
- **Impact:** Major — important checks are easy to skip and validation differs between contributors.
- **Fix:** Add scripts such as `typecheck`, `lint`, `test`, `test:rust`, `audit`, and `verify`; have `verify` run formatting checks, frontend tests/build, Rust tests/Clippy, and audits in a stable order with clear failures.
- **Effort:** S
- **Grade lift:** C+ → B− (creates a repeatable local quality gate)

#### ~~I2~~ ✓ done 2026-08-27 — Enforce Rust and frontend formatting/linting
- **Where:** Broad Rust formatting drift under `draft-assistant/src-tauri/src/`; no ESLint/Prettier config exists; `draft-assistant/package.json:18-24`
- **What's wrong:** `cargo fmt --check` fails today across multiple Rust modules, while React/TypeScript has no semantic linter or consistent formatter beyond the compiler.
- **Impact:** Moderate — avoidable style churn obscures real diffs, and React-specific mistakes are not checked.
- **Fix:** Apply rustfmt in a dedicated change, add ESLint with TypeScript and React Hooks rules plus a formatter, expose check/fix scripts, and gate check mode in CI.
- **Effort:** M
- **Grade lift:** C+ → B− (makes source consistency automatic and catches frontend-specific defects)

#### I3 — Pin Node, npm, and Rust toolchains
- **Where:** No `.nvmrc`, `.node-version`, Volta config, or `rust-toolchain.toml` exists; build expectations begin at `draft-assistant/README.md:37-49`
- **What's wrong:** The project does not declare the runtime/toolchain versions that produced its lockfiles and successful builds.
- **Impact:** Moderate — contributors and CI can receive different compiler, lockfile, or platform behavior with no obvious cause.
- **Fix:** Add explicit supported Node/npm and Rust stable versions, document the update policy, and make CI use the same pins.
- **Effort:** S
- **Grade lift:** C+ → B− (makes the local and automated build environments reproducible)

#### I4 ◐ partial 2026-08-27 — Add CI with cached, target-aware jobs
- **Where:** No `.github/workflows/` directory exists
- **What's wrong:** No automated system runs the otherwise useful build, tests, Clippy, audit, or packaging checks. Rust's 4.2 GB local target tree also shows the value of intentional caching and cleanup policy.
- **Impact:** Major — regressions and platform-specific failures have no pre-merge signal.
- **Fix:** Add macOS-focused CI for the currently shipped platform, cache npm/Cargo registries and build outputs with lockfile keys, run the all-up verification command, and add separate scheduled cross-target/advisory checks for future platforms.
- **Effort:** M
- **Grade lift:** C+ → B+ (makes the strong local toolchain consistently enforceable)
- **Progress:** Added the macOS verification job with npm caching. Cargo build caching and separate scheduled cross-target/advisory jobs remain.

---

# Verification addendum — Claude, 2026-08-27

Every substantive claim above was re-checked against the source. This section
records what held up, what did not, and what the audit missed. Method: direct
code reads plus `cargo tree --target aarch64-apple-darwin`, `cargo audit`,
`cargo fmt --check`, `npm audit`, `npm outdated`, and a state dump against a
live Sleeper mock draft.

## Confirmed — no correction needed

| Item | Verification |
|------|-------------|
| B1 manual picks lost on reload | Confirmed. `load_league`/`assemble` construct `manual_picks: Vec::new()` on every path; `refresh_data` replaces the whole `LoadedLeague`. |
| B3 poller swallows failures | Confirmed. `lib.rs` poll loop discards `Err` from both `picks()` and `draft()`; no health field exists in `DataHealth`. |
| B4 persistence errors discarded | Confirmed. `write_cache`/`save_config` drop every I/O error via `.ok()`. |
| E1 `csp: null` | Confirmed verbatim in `tauri.conf.json:23`. |
| E2 opener plugin unused | Confirmed. Zero references to `opener` anywhere under `src/`; still declared in `package.json`, `Cargo.toml`, and `capabilities/default.json`. |
| G2 no HTTP timeout | Confirmed. `SleeperClient::new` sets only `user_agent` and `gzip`; reqwest applies no default timeout. |
| I2 `cargo fmt --check` fails | Confirmed — 39 files differ. |
| A2 hand-mirrored TS types | Confirmed. `types.ts` is manual and `api.ts:53` casts fixture JSON with no validation. |
| Validation snapshot numbers | Reproduced exactly: 13 Rust tests pass, clippy clean, npm audit 0/133, bundle 206.63 kB / 64.82 kB gzip. |

## Corrections

### ❌ F1 is a false positive — the flagged crates are not in this build

The report grades the `glib` unsoundness advisory (RUSTSEC-2024-0429,
RUSTSEC-2025-0098) as **Major** supply-chain risk. Those are Linux GTK crates,
pulled in by Tauri's Linux backend. On the shipped target they are not compiled:

- `grep -cE '^name = "(glib|gtk|gdk)' Cargo.lock` → **13** crates present in the lockfile
- `cargo tree -e normal --target aarch64-apple-darwin | grep -ciE 'glib|gtk|gdk'` → **0** in the macOS build graph

This is a lockfile artifact, not shipped code. macOS Tauri uses WKWebView. The
item is still worth a two-line `audit.toml` ignore policy so real advisories
stay visible, but its severity is **Minor**, not Major, and it does not belong
in any priority list.

### ❌ D4 / I4 (CI) are graded against a team that does not exist

Both are marked Major for the absence of CI. This project has **no git
repository at all** (`git status` → `fatal: not a git repository`), no remote,
and one contributor. CI has nothing to gate and no event to trigger on. The
real finding underneath is far more serious and is missed entirely — see
**New-1** below.

### ❌ F2 (Vite 8 / TypeScript 7 upgrades) is graded backwards for this project

Confirmed the version gaps are real (`@vitejs/plugin-react` 4.7.0 → 6.1.0,
Vite 7.3.6 → 8.2.2, TypeScript 5.8.3 → 7.0.2). But this app has a hard,
non-negotiable deadline of **2026-08-28 17:00 PDT**, no tests covering the
frontend, and no version control to revert with. Running three major toolchain
migrations in that state is a net *increase* in risk. Correct grade for acting
on this before the draft: **do not**. Revisit afterward.

### ⚠️ B2 is right that kickers are broken, but wrong about how

The report states kickers receive RB/WR depth heuristics via the `_ =>` arm in
`recommend.rs`. That code path is unreachable, because **kickers never enter the
board at all**: both projection URLs in `sleeper.rs:230,242` request only
`position[]=QB|RB|WR|TE|DEF`. A live state dump of a K-slot mock draft returns
`{WR: 138, RB: 95, TE: 58, QB: 46, DEF: 32}` — zero kickers out of 369 players.

The actual defect is worse than described: in a league with a `K` slot,
`board.rs:48` adds `K` to `wanted`, the slot is created, and it can **never be
filled**. It stays permanently in `open_starters`, inflating `need_pressure` in
`recommend.rs` for the entire draft. The superflex/mixed-flex half of B2 is
confirmed accurate (`valuation.rs:80` uses `flex_slots[0]` for all flex demand,
with an in-code comment acknowledging it).

**Not applicable to the UMass league** (no kicker, single flex type). It became
reachable today when mock-draft support was added, since Sleeper mocks default
to a kicker slot.

### ⚠️ Category D understates what exists

Testing is graded **C−** on "no integration tests." A 210-pick autopilot
simulation plus an invariant validator (no duplicate picks, snake ownership,
drafted ∉ available, survival ∈ [0,1], roster counts, no DEF before round 13)
does exist and runs end to end through the real `DraftView` — it is
`bin/dump_state.rs --simulate N`, a binary rather than a `#[test]`. The correct
criticism is **"the integration harness is not wired into `cargo test`, so it
cannot catch regressions automatically,"** not that the data path is unverified.
C− is roughly the right letter; the justification is not.

### ⚠️ Optimistic grade lifts

E1 claims one CSP line moves Security B− → B+. Adding a CSP to an app whose
entire remote surface is two hard-coded HTTPS GET origins is worth roughly half
that. Several other lifts are similarly generous; treat the letters as
directional.

## Missed — the two largest actual risks

### New-1 — There is no version control (severity: Critical)

`git status` fails at the project root. ~3,700 lines of hand-written source, on
a **removable flash volume** (`/Volumes/512Flash`), with no history, no
branches, no backup, and no way to revert a bad edit. Every refactor this report
recommends — all L-effort, several touching the live draft path — would be
performed with no undo, the day before the draft.

This dominates every other item in the report. It is 30 seconds of work:

```bash
cd /Volumes/512Flash/Draft-app && git init && git add -A && git commit -m "Working draft assistant before hardening"
```

The audit graded the absence of *CI* as Major twice (D4, I4) while never
noticing there is nothing for CI to run against.

### New-2 — A stale cache plus a dead endpoint bricks the app (severity: Major)

`engine.rs:80-87` — `read_cache` returns `None` once past TTL (players 24h,
projections 6h). `engine.rs:222-224` then propagates fetch failure with `?`.
There is **no fallback to stale cache**. So if the undocumented projections
endpoint is unavailable when the cache has expired, `load_league` fails outright
and the app cannot open a league — no board, no recommendations, nothing.

The projections endpoint is undocumented and unversioned; it is a single point
of failure carrying the entire valuation model, and it has no fallback path and
no schema-drift detection. On draft day the cache TTL is 6 hours, so an outage
at 4 PM with a cache fetched at 9 AM produces exactly this failure at exactly
the wrong moment.

**Fix:** on fetch failure, fall back to stale cache and surface the staleness in
`DataHealth` rather than failing the load.

## Corrected priority order

The report's "Top 5 = B1, D1, B2, D2, E1" front-loads test infrastructure and
puts the two items that can actually break draft night at #3 and #5. Ordered by
real risk for this project:

1. **New-1** — `git init` (30 seconds, unblocks safely doing anything else)
2. **New-2** — stale-cache fallback (S)
3. **G2** — HTTP timeouts (S)
4. **B3** — make a failed sync visibly change the banner instead of lying (M)
5. **B1** — persist manual picks across refresh (M)

Everything else — CSP, accessibility, flex/superflex generalization, CI,
formatting, toolchain majors — is genuine and worth doing, but **after** the
2026-08-28 draft, not before it.

## Overall grade assessment

**C+ is too harsh for what this is.** The report applies a production-SaaS
rubric to a single-user, local-first, read-only desktop tool with one
stakeholder and a one-night deadline. Adjusting the three false positives,
restoring credit for the simulation harness, and weighting by actual user-facing
risk puts it at **B−**. The findings are well-researched and the fix
descriptions are specific and correct; the *calibration* and the *ranking* are
what need revising.

See `.claude/grade-report.md` for the independent Claude audit that starts from
this report as its baseline.
