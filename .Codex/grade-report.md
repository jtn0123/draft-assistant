# Codebase Grade Report

**Project:** Draft Assistant 0.3.1
**Audited:** 2026-09-08
**Source:** local `main` at `8bad391`; clean at audit start. GitHub `origin/main` is `3ef3cdb` after fetch: local is one commit ahead, zero behind.
**Stack:** Tauri 2, Rust/Tokio, React 19, TypeScript, Vite; plain JavaScript mobile companion.

This full regrade replaces the September 7 report at the user's request. It grades current source, including the later mobile work and streaming/command tests. No application fixes, push, or live league changes are part of this audit. The previous B+ was a narrower draft-readiness assessment; today's B includes newly validated defects and the expanded phone surface.

## Authorized remediation — 2026-09-08

All 15 findings are implemented in the local working tree. The grade table below is the original audit baseline, not a new post-fix regrade. No commit, push, installed-app replacement, real-device revocation, or live league change was performed.

| IDs | Completed behavior |
|---|---|
| E1 | Revocation immediately ends live access and reports storage failure honestly; successful retry survives host restart. Failure-path tests use a fake secret store. |
| E2 | Audit error JSON, incomplete reports and process failures fail the dependency gate. |
| B1 | Missing projection inputs show unavailable odds/ranks; exact simulation ties share credit. |
| B2 | Identity changes become active only after their ordered settings save succeeds. |
| C1, C2 | Phone player details refresh when displayed fields change; modal focus skips disabled/hidden controls and handles an empty focus list. |
| G1 | Desktop and remote snapshots build off-lock on the blocking pool; remote response encoding also runs off async workers. |
| A1 | Draft DTOs and schema constants are generated from Rust; Platform is a serialized closed enum. CI checks generated output, and unsupported serialization syntax fails generation. Draft schema is now 1.8. |
| D1, D3 | CI runs Chromium/WebKit phone tests with retained failure evidence, early runtime-error capture and deterministic local image fixtures. |
| D2 | Separate Istanbul instrumentation measures all 15 shipped phone scripts with enforced coverage floors. Omitting one regression demonstrably reduces coverage. |
| I1 | Shipped companion JavaScript receives recommended ESLint rules; an injected undefined name correctly fails the gate. |
| F1 | The E2E extraction exception now records trusted archive sources, a September 8 review, an October 8 deadline and upgrade-triggered rechecks. The upstream vulnerability remains unpatched; this finding's exception-management fix is complete. |
| H1, H2 | READMEs describe automatic AI routing and clean-Mac prerequisites; TESTING documents generated contracts, phone coverage and mobile CI. |

**Final verification:** Passed: 1,052 frontend tests, 137 companion coverage tests, 1,451 Rust tests, 51 script tests, 42 production-browser checks (41 in the full run plus the corrected projections case, with both projections tests passing on rerun), and 4 phone-browser checks. Fast gate, generated-contract check, dependency policy and production build passed. The full verify run exposed an old replay-contract test that assumed inline constants; after correcting that test, the complete Rust coverage run and production build passed separately. Frontend coverage: 95.68% lines / 89.19% branches / 91.07% functions. Companion: 94.89% lines / 83.11% branches / 91.05% functions, with enforced 90/78/86 floors (88 statements). Rust: 93.47% lines / 90.62% functions, above the 90/87 floors.

The current source was tested locally. Native UI, physical-phone/Tailscale sessions and live AI calls were not repeated for this batch. The service worker remains an explicitly uncovered file in the phone report; aggregate coverage is not a claim that every phone feature was exercised.

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B+ | 1 |
| B | Backend Quality | B | 2 |
| C | Frontend Quality | B | 2 |
| D | Testing & Reliability | B+ | 3 |
| E | Security | B− | 2 |
| F | Dependencies & Tech Currency | B | 1 |
| G | Performance & Scalability | B | 1 |
| H | Documentation & Onboarding | B | 2 |
| I | Developer Experience & Tooling | B+ | 1 |
| **Overall** | | **B** | **15** |

**Top 5 highest-leverage fixes:** E1, E2, B1, C1, D1

The app is a usable draft helper with broad passing checks, but not an A-level reference implementation. Prioritize durable device revocation, honest projections and trustworthy automated gates. Continue submitting actual picks in Sleeper.

## Audit-baseline verification and limits

- Current fast gate passed: LOC, CSS, text, formatting, ESLint, Clippy and TypeScript. Guard-script tests passed.
- Current frontend: 1,048 tests in 97 files passed; 95.66% line, 89.14% branch and 91.07% function coverage. These percentages exclude shipped companion JavaScript (D2).
- Current production browser suite: 42 passed. Current mobile suite: 4 passed across iPhone WebKit and Android Chromium, light and dark. Mobile screenshots were inspected by the frontend reviewer.
- Current `npm run verify`: exit 0, including 1,441 Rust tests and the production build. Rust coverage: 93.44% lines / 90.61% functions, above the 90% / 87% gates.
- Live dependency checks: app npm 0 vulnerabilities; E2E 13 high dependency entries from one allowlisted advisory; Cargo 0 non-ignored vulnerabilities and 6 unmaintained warnings. Passing the configured policy is not a claim of no advisories.
- GitHub CI and native End-to-end succeeded for published `3ef3cdb` ([CI](https://github.com/jtn0123/draft-assistant/actions/runs/34190669916), [native](https://github.com/jtn0123/draft-assistant/actions/runs/34190669912)). The unpublished `8bad391` has no hosted run.
- No fresh native app launch, physical-phone/Tailscale session, live AI turn, full-draft soak or performance benchmark was performed. Earlier physical-device success remains historical evidence, not a test repeated today.
- Findings B1, C1 and E2 have scratch reproductions; E1/B2/G1 were validated from direct control flow. No application source was modified for reproductions.

Dependency primary source: [GitHub GHSA-jmr9-qjv8-65gv](https://github.com/advisories/GHSA-jmr9-qjv8-65gv), checked today, lists extract-zip through 2.0.1 as affected with no patched version.

---

## A — Architecture & Design — B+

Domain boundaries separate polling from orchestration (`draft-assistant/src-tauri/src/poll.rs:1`) and share application state across desktop and companion (`src-tauri/src/state.rs:15`). Season processing snapshots inputs before off-thread work (`src-tauri/src/state.rs:281`). Independently maintained wire types still permit requiredness drift.

#### ~~A1~~ ✓ done 2026-09-08 — Make serialized view types a single-source contract
- **Where:** draft-assistant/src-tauri/src/view_types.rs:120–129; draft-assistant/src/types.ts:196–214; draft-assistant/src/api.ts:24–43; draft-assistant/src-tauri/tests/fixture_shape.rs:70–95
- **What's wrong:** Rust requires draft_projections, while TypeScript declares it optional. The runtime guard checks a version string; fixture-shape checks do not validate TypeScript types or requiredness.
- **Impact:** Moderate — consumers can compile against a different understanding of the same payload.
- **Fix:** Generate TypeScript DTOs and schema constants from the Rust serialized contract and check generated output in CI. Retain fixture-shape tests.
- **Effort:** M
- **Grade lift:** B+ → A− — removes a demonstrated source of contract drift.

---

## B — Backend Quality — B

Manual draft edits validate inputs and roll back failed writes (`src-tauri/src/commands_draft/edits.rs:37–92`). Polling protects against stale replies, and configuration writes are ordered and atomic (`src-tauri/src/engine/config.rs:195–220,275–306`). Projection output and identity changes still have concrete failure cases.

#### ~~B1~~ ✓ done 2026-09-08 — Handle missing projection data honestly
- **Where:** draft-assistant/src-tauri/src/draft_projection.rs:173–199; draft-assistant/src-tauri/src/season_spread.rs:170–172; draft-assistant/src-tauri/src/draft_projection_tests.rs:165–170
- **What's wrong:** With zero means and zero spreads, the strict winner comparison awards every simulation to the first roster. Running the unchanged simulation in a temporary harness with three empty rosters returned [1.0, 0.0, 0.0].
- **Impact:** Major — the app displays an unsupported 100% favorite before useful projection data exists.
- **Fix:** Represent insufficient projection data explicitly and display unavailable odds until meaningful input exists. Split exact ties fairly; test multiple empty rosters, missing projections and deterministic ties.
- **Effort:** S
- **Grade lift:** B → B+ with B2 — removes misleading numerical output.

#### ~~B2~~ ✓ done 2026-09-08 — Keep identity unchanged when saving fails
- **Where:** draft-assistant/src-tauri/src/commands_draft.rs:194–210; draft-assistant/src-tauri/src/commands_chat.rs:215–220
- **What's wrong:** The identity setter mutates live config before its fallible save and propagates the error without rollback. A failed save can change the active identity despite telling the UI it failed.
- **Impact:** Moderate — the selected roster can change in memory and revert after restart.
- **Fix:** Use a transactional configuration helper that preserves save ordering and commits or safely rolls back the affected field. Test failed persistence and unchanged in-memory identity.
- **Effort:** M
- **Grade lift:** B → B+ with B1 — aligns error results with actual state.

---

## C — Frontend Quality — B

Settings has clear sections and accessible controls (`src/components/SettingsPage.tsx:6–36,46–77`); model pickers collapse and restore focus (`src/components/ChatControls.tsx:49–66`, `src-tauri/companion-static/models.js:71–79`). Lazy screens have loading/error boundaries (`src/components/lazyScreens.tsx:10–32`). Current phone browser screenshots are readable, but stale rows and modal keyboard handling need correction.

#### ~~C1~~ ✓ done 2026-09-08 — Keep mobile available-player details current
- **Where:** draft-assistant/src-tauri/companion-static/available.js:47–71,162–175
- **What's wrong:** The render signature omits displayed name, position, team and bye. A JSDOM reproduction loading the actual modules changed team OLD to NEW and bye 1 to 9 while the DOM retained OLD and Bye 1.
- **Impact:** Moderate — refreshed player details can remain stale on the phone.
- **Fix:** Include all displayed fields in the signature or compare complete display data. Add a regression changing only team and bye.
- **Effort:** S
- **Grade lift:** B → B+ with C2 — fixes stale mobile presentation.

#### ~~C2~~ ✓ done 2026-09-08 — Wrap modal focus around enabled, visible controls
- **Where:** draft-assistant/src/components/useFocusTrap.ts:5–6,33–44; draft-assistant/src/components/Overlays.tsx:76–88
- **What's wrong:** The focus trap includes disabled and hidden controls. During manual-pick submission, Tab from Cancel prevents default then attempts to focus disabled Mark drafted, leaving keyboard navigation stalled.
- **Impact:** Moderate — keyboard interaction breaks while a dialog operation is busy.
- **Fix:** Filter to enabled, visible, focusable descendants. Focus the dialog when none remain. Test disabled first/last and hidden controls.
- **Effort:** S
- **Grade lift:** B → B+ with C1 — makes shared keyboard behavior reliable.

---

## D — Testing & Reliability — B+

The current suite covers cancellation, the command-name contract and Tauri progress events (`src/chatCancel.test.ts`, `src/commandNames.test.ts`, `src-tauri/src/commands_chat_progress_tests.rs`). CI enforces Rust/frontend coverage and production browser checks (`.github/workflows/ci.yml:94–116`). The phone has substantial behavioral tests, but its browser gate and coverage measurement have gaps.

#### ~~D1~~ ✓ done 2026-09-08 — [FE] Run the mobile browser suite in CI
- **Where:** draft-assistant/playwright.config.ts:42–44; draft-assistant/playwright.mobile.config.ts:15–27; .github/workflows/ci.yml:105–116
- **What's wrong:** The main browser config excludes companion-mobile.spec.ts. No workflow invokes test:e2e:mobile, so the only WebKit phone walkthrough can regress while CI remains green.
- **Impact:** Moderate — the phone experience has no continuous browser gate.
- **Fix:** Install WebKit and Chromium in a dedicated mobile CI job, run test:e2e:mobile, and retain failure screenshots/traces. Keep the desktop production-bundle suite.
- **Effort:** S
- **Grade lift:** B+ → A− with D2/D3 — continuously verifies both supported phone browser engines.

#### ~~D2~~ ✓ done 2026-09-08 — [FE] Measure companion JavaScript coverage separately
- **Where:** draft-assistant/vitest.config.ts:20–28; draft-assistant/src/test/companionPageHarness.ts:19–36
- **What's wrong:** The coverage include list only covers src TypeScript/TSX. Shipped companion-static JavaScript is evaluated through a VM harness and does not contribute to those coverage floors.
- **Impact:** Moderate — desktop coverage percentages do not measure the growing phone application.
- **Fix:** Instrument the shipped companion scripts with preserved filenames, add a separate companion coverage report and a measured baseline floor. Verify deleting a companion regression lowers that report.
- **Effort:** M
- **Grade lift:** B+ → A− with D1/D3 — makes reported coverage representative of both clients.

#### ~~D3~~ ✓ done 2026-09-08 — [FE] Make the phone walkthrough detect runtime errors reliably
- **Where:** draft-assistant/e2e-browser/companion-mobile.spec.ts:57–59,83–92,144–146
- **What's wrong:** The pageerror listener is installed only after the walkthrough and immediately asserts an empty array, so it misses all earlier errors. The walkthrough also requires a real CDN image despite its comment saying images are not asserted.
- **Impact:** Moderate — the test can miss runtime errors or fail because of an unrelated CDN outage.
- **Fix:** Register pageerror collection before navigation and assert at the end. Fulfill image requests with a checked-in image for deterministic layout checks; keep any live-CDN smoke check separately identified.
- **Effort:** S
- **Grade lift:** B+ → A− with D1/D2 — removes a vacuous assertion and external flake source.

---

## E — Security — B−

Companion routes authenticate bearer tokens and expose an explicit config subset (`src-tauri/src/companion/routes.rs:160–180,294–321`). Pairing has global and per-address throttles (`pairing.rs:65–82`), tokens use Keychain storage, and CLI tool use is disabled (`chat_cli.rs:263–274`, `chat_codex.rs:167–204`). Persistence failure in revocation and a fail-open audit wrapper prevent a higher grade.

#### ~~E1~~ ✓ done 2026-09-08 — Make device revocation durable or report failure
- **Where:** draft-assistant/src-tauri/src/companion/store.rs:180–200; draft-assistant/src-tauri/src/companion/hub.rs:104–132,159–177,272–287
- **What's wrong:** Revoke clears in-memory devices, calls persistence and returns success, while the storage layer swallows write failures. If a Keychain write fails, old stored tokens survive and startup reloads them.
- **Impact:** Major — restarting can restore access to a device the user was told was revoked.
- **Fix:** Propagate storage failure through save, persist and revoke, and surface failed durable revocation. Ensure startup cannot restore tokens invalidated by a recorded revocation. Test initial successful persistence, failed revocation persistence, and hub reconstruction.
- **Effort:** M
- **Grade lift:** B− → B with E2 — makes revocation results trustworthy across restarts.

#### ~~E2~~ ✓ done 2026-09-08 — Fail the dependency gate when npm cannot audit
- **Where:** draft-assistant/scripts/check-npm-audit.mjs:23–25,58–69,86–95
- **What's wrong:** The wrapper accepts any JSON stdout and interprets missing vulnerabilities as clean. A scratch npm emitting ENOAUDIT error JSON and exiting 1 made the real checker return 0 and report both trees clean.
- **Impact:** Major — CI can claim a security audit passed when it never ran.
- **Fix:** Reject error JSON and malformed/incomplete reports; distinguish vulnerability exit status from operational failure. Test JSON errors, missing metadata/vulnerabilities, spawn failures and valid reports.
- **Effort:** S
- **Grade lift:** B− → B with E1 — restores a reliable security check.

---

## F — Dependencies & Tech Currency — B

Both npm trees and Cargo have locks, with Dependabot coverage (`.github/dependabot.yml`). Today's app npm audit reports zero vulnerabilities; the E2E tree has 13 high dependency entries tracing to one allowlisted extract-zip advisory. Cargo reports zero non-ignored vulnerabilities and six unmaintained warnings under the configured policy, which explicitly ignores Linux glib advisory RUSTSEC-2024-0429.

#### ~~F1~~ ✓ done 2026-09-08 — Bound and review the E2E extraction exception
- **Where:** draft-assistant/scripts/npm-audit-allowlist.json:4; draft-assistant/e2e/package-lock.json; .github/dependabot.yml:36–50
- **What's wrong:** The E2E tree retains a high-severity symlink traversal advisory with a reasoned exception but no review date or explicit archive-source constraints. This is a development/test dependency, not a shipped npm dependency.
- **Impact:** Moderate — test tooling still uses a known vulnerable extractor.
- **Fix:** Document trusted browser/driver archive sources and a review date. Track upstream replacement, recheck on WDIO upgrades, and remove the exception when the chain is clean. Avoid blindly accepting a suggested major downgrade.
- **Effort:** S
- **Grade lift:** B → B+ when the vulnerable chain is replaced; a time-bounded exception improves control meanwhile.

---

## G — Performance & Scalability — B

Season caching and event suppression reduce repeated work (`src-tauri/src/poll.rs:210–338`); immutable inputs are shared with Arc before blocking-pool processing (`src-tauri/src/state.rs:281–348`). The draft poll path also builds outside locks. Several direct snapshot paths still perform expensive work under shared locks; no fresh latency benchmark was run.

#### ~~G1~~ ✓ done 2026-09-08 — Build direct and remote draft snapshots outside locks
- **Where:** draft-assistant/src-tauri/src/companion/routes.rs:249–255; draft-assistant/src-tauri/src/companion/ws.rs:213–218; draft-assistant/src-tauri/src/commands_draft.rs:238–243; draft-assistant/src-tauri/src/commands_draft/tick.rs:232–255; draft-assistant/src-tauri/src/view.rs:235–258
- **What's wrong:** Direct reads and companion connection snapshots hold loaded/config guards while building the draft view, including simulations and player copies. HTTP also serializes while holding the guards, unlike the repaired polling path.
- **Impact:** Moderate — simultaneous phone reconnects can unnecessarily block polling and commands.
- **Fix:** Move the existing off-lock builder into shared state utilities. Snapshot inputs under locks, release guards, then build and serialize on the blocking pool; preserve caller identity checks. Measure concurrent snapshot/poll latency.
- **Effort:** M
- **Grade lift:** B → B+ — extends the existing performance approach to remaining entry points.

---

## H — Documentation & Onboarding — B

The app README explains polling, caching, architecture and release behavior (`draft-assistant/README.md:152–245,247–345,374–404`). Root documentation covers Tailscale/HTTPS, and TESTING distinguishes fixtures from physical-device evidence (`draft-assistant/TESTING.md:22–29,104–123`). AI setup text is stale and clean-machine prerequisites are missing.

#### ~~H1~~ ✓ done 2026-09-08 — Match AI setup instructions to automatic routing
- **Where:** draft-assistant/README.md:87–88,354–365; draft-assistant/src/components/ChatControls.tsx:4–6; draft-assistant/src-tauri/src/commands_chat.rs:86–96
- **What's wrong:** README directs users to choose a provider or Claude connection in a picker that was removed. Installed Claude CLI now takes precedence automatically and the stored provider preference is ignored.
- **Impact:** Moderate — authentication and billing troubleshooting points to a nonexistent control.
- **Fix:** Explain model selection separately from automatic provider routing, CLI precedence and the API-key fallback. Remove the obsolete picker instructions.
- **Effort:** S
- **Grade lift:** B → B+ with H2 — restores accurate AI onboarding.

#### ~~H2~~ ✓ done 2026-09-08 — Document prerequisites for a clean development Mac
- **Where:** README.md:10–16; draft-assistant/README.md:91–104
- **What's wrong:** Quick starts jump to npm installation and tauri dev without listing Node/npm, Rust or Apple developer tool prerequisites.
- **Impact:** Moderate — new contributors need independent troubleshooting before the documented setup works.
- **Fix:** List prerequisites using repository-supported toolchain versions, installation links and verification commands. Distinguish browser preview from native build requirements.
- **Effort:** S
- **Grade lift:** B → B+ with H1 — makes setup reproducible.

---

## I — Developer Experience & Tooling — B+

The repository enforces LOC/CSS rules, formatting, strict TypeScript/React lint and Clippy (`draft-assistant/package.json:6–44`, `draft-assistant/eslint.config.js:32–61`). Pre-commit resolves the actual worktree before verify:mid (`.githooks/pre-commit:8–13`), and workflows pin action SHAs. The companion JavaScript has formatting but no meaningful ESLint rule set.

#### ~~I1~~ ✓ done 2026-09-08 — Lint the shipped companion JavaScript
- **Where:** draft-assistant/eslint.config.js:13–33; draft-assistant/src-tauri/companion-static/*.js
- **What's wrong:** ESLint config blocks cover scripts/configs, browser tests and src TypeScript, but not the shipped phone scripts. eslint --print-config for companion-static/app.js returned an empty rules object.
- **Impact:** Moderate — lint passes while common JavaScript defects in the phone client are unchecked.
- **Fix:** Add an ESLint block for companion-static scripts with recommended rules, browser globals and the correct classic-script source type. Declare intentional cross-file globals explicitly; verify an undefined identifier is caught.
- **Effort:** S
- **Grade lift:** B+ → A− — applies the existing quality gate to the phone application.
