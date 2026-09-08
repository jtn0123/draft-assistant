# Codebase Grade Report — draft-night readiness

**Project:** Draft Assistant 0.3.1
**Audited:** 2026-09-07
**Source:** `d7a0bfe` (application code unchanged from the initial `08b1950` snapshot; intervening commit was TRACKER documentation)
**Stack:** Tauri 2, Rust/Tokio, React 19, TypeScript, Vite.

This is the historical pre-remediation audit snapshot. The user subsequently approved replacing the August 27 report and implementing all seven findings. See [the current grade report](grade-report.md) for the final implementation and readiness verdict. Findings below describe current source, not the old grade or tracker assertions. Source files were not changed in this audit.

## Summary

| ID | Category | Grade | Items |
|---|---|---|---|
| A | Architecture & Design | B+ | 0 |
| B | Backend Quality | B | 1 |
| C | Frontend Quality | B | 2 |
| D | Testing & Reliability | B | 1 |
| E | Security | B+ | 0 |
| F | Dependencies & Tech Currency | B+ | 0 |
| G | Performance & Scalability | B− | 1 |
| H | Documentation & Onboarding | B | 1 |
| I | Developer Experience & Tooling | B+ | 1 |
| **Overall** | | **B** | **7** |

**Top 5 highest-leverage fixes:** G1, B1, C1, D1, C2. C2 is a requested feature gap and should follow draft reliability fixes.

## Tonight's verdict

Conditional go as a read-only helper for a conventional Sleeper snake draft; not yet a fully rehearsed replacement for the Sleeper draft room. Make picks in Sleeper. The saved active league's live API reports a 2026 snake draft, status `pre_draft`, start timestamp `1788836451000`: September 7 at **20:00:51 America/Los_Angeles (PDT)**, equivalent to **19:00:51 fixed PST**. If the intention was 7 PM local clock time, the stored platform schedule differs by one hour. No league schedule was changed.

The pre-existing release bundle in `draft-assistant/src-tauri/target/release/bundle/macos/Draft Assistant.app` was version **0.1.0**, binary dated August 27, whereas source is **0.3.1**. No copy was found at `/Applications/Draft Assistant.app`. A grade for current source is not evidence for that old bundle.

Scope limits: auction order/survival advice is deliberately withheld (`src-tauri/src/draft.rs:51`, `view.rs:134`); IDP slots are excluded with warnings (`engine_assemble.rs:96`). The actual active league's player/projection freshness, user's confirmed seat, and full draft rehearsal remain separate from passing automated tests.

## Validation

- Static LOC, CSS, text, formatting, lint, Rust clippy and TypeScript checks passed in `npm run verify`.
- 48 script guard tests passed.
- 951 frontend tests in 81 files passed; frontend line coverage 95.60%, branches 89.28%.
- All 38 production Chromium tests passed (`PW_PORT=1437 npm run test:e2e:browser:ci`), without retries.
- `npm run audit` passed its configured policy. This means no unaccepted high/critical npm advisories, not zero advisories. The allowlist explicitly accepts an extract-zip advisory in the native-test dev tree; Cargo ignores one Linux-only glib unsoundness advisory and reports 16 unmaintained transitive warnings.
- Installed Claude Code was authenticated. A live tools-disabled, no-session-persistence call using `claude-opus-5`, low effort and the app's JSON CLI argument shape returned `Draft chat connection OK`, without an error. This verifies the CLI provider connection, not an in-app UI round trip or the separate Anthropic API-key route.
- Full Rust coverage gate and native WKWebView smoke test passed; see final validation addendum below.
- Current-head hosted CI and End-to-end workflows were running at inspection; no open PRs were returned. No merge, push or release was performed.

## A — Architecture & Design — B+

Domain rules are separate from commands and transports (`src-tauri/src/draft.rs`, `recommend.rs`, `commands_draft/tick.rs`). The poll loop builds views from snapshots outside shared locks (`commands_draft/poll_loop.rs:227`), and chat transport dispatch has its own module (`commands_chat_route.rs`). No additional architectural refactor is justified before tonight.

## B — Backend Quality — B

The backend has explicit malformed-response refusal and manual-pick reconciliation (`commands_draft/refusal.rs`, `commands_draft/poll_loop.rs`). League-switch guards protect adoption of network replies, but view change detection omits some meaningful updates.

#### B1 — Publish draft order/settings and pick metadata changes
- **Where:** `draft-assistant/src-tauri/src/poll.rs:52`, `draft-assistant/src-tauri/src/commands_draft/poll_loop.rs:157`, `draft-assistant/src-tauri/src/commands_draft/poll_loop.rs:227`.
- **What's wrong:** Pick signatures include pick number and player ID only; draft changes compare status only. Updated order/settings are stored without triggering a rebuilt view. Same-player keeper or draft-slot changes can likewise remain invisible until another detected change or manual refresh.
- **Impact:** Moderate — commissioner edits may leave draft advice or clock details stale.
- **Fix:** Include view-relevant draft order/settings and pick metadata in change detection. Test same-status order/timer changes and same-player keeper metadata changes.
- **Effort:** S
- **Grade lift:** B → B+ (closes an observable stale-state path).

## C — Frontend Quality — B

Setup has loading/error states, draft tables have filtering/paging, and browser tests cover narrow-window layout and replay ordering (`src/components/Board.tsx`, `e2e-browser/draft-board.spec.ts`). The chat UI has a session lifecycle race and does not implement the requested OpenAI providers. No broad visual redesign is needed to address these findings.

#### C1 — Prevent a new chat from inheriting a pending old answer
- **Where:** `draft-assistant/src/components/Chat.tsx:235`, `draft-assistant/src/components/Chat.tsx:273`, `draft-assistant/src/components/useChatThread.ts:115`, `draft-assistant/src/components/useChatSessions.ts:118`.
- **What's wrong:** New remains enabled during an answer. Fresh start clears the thread and changes its session ID, but the old completion restores its old history and calls the latest save callback, filing that history under the new session too. A temporary deferred-answer regression reproduced the UI overwrite: Send → New → Fresh start → resolve old answer. The assertion that the old answer stays absent failed. The temporary test was removed; existing source/tests were not edited.
- **Impact:** Moderate — a new conversation can unexpectedly become a duplicate of the old one.
- **Fix:** Disable New and both new-chat choices while sending, guard both handlers, and add a deferred-answer regression including opening the choice bar before Send.
- **Effort:** S
- **Grade lift:** B → B+ (protects conversation lifecycle).

#### C2 — Add OpenAI chat support if Astra/Sol are required
- **Where:** `draft-assistant/src/chat-types.ts:30`, `draft-assistant/src/components/ChatControls.tsx:10`, `draft-assistant/src-tauri/src/chat_types.rs:10`, `draft-assistant/src-tauri/src/commands_chat_route.rs:43`.
- **What's wrong:** The UI and backend offer Opus 5/Fable 5 through Claude Code or Anthropic API only. There is no OpenAI route or Astra/GPT-5.6 Sol picker.
- **Impact:** Moderate — the requested provider capability is absent; this is a feature gap rather than a broken Claude path.
- **Fix:** Add an explicitly selected OpenAI transport, authentication/settings, requested models, response/usage normalization, cancellation and error handling; test dispatch and one real app round trip. Retain existing Claude routes. Choose subscription-backed Codex versus API credentials explicitly before implementation.
- **Effort:** M
- **Grade lift:** B → B+ (fulfills requested chat capability; not independently a reliability improvement).

## D — Testing & Reliability — B

Unit and integration tests cover cancellation, wire retries, draft rules, companion authentication and replay ordering (`src-tauri/src/chat_wire_retry_tests.rs`, `src-tauri/tests/league_rules.rs`, `src-tauri/tests/companion/`, `e2e-browser/draft-board.spec.ts`). Coverage floors are enforced in `package.json`. Native end-to-end coverage remains much narrower than those suites.

#### D1 — Rehearse critical draft interactions in the native app
- **Where:** `draft-assistant/e2e/specs/smoke.e2e.ts:25`, `draft-assistant/playwright.config.ts:4`.
- **What's wrong:** The sole native smoke test proves boot to a resolved setup/header screen. Browser tests stub Tauri IPC; neither proves native league selection, advancing picks, manual fallback/undo and in-app chat together.
- **Impact:** Moderate — green suites alone do not prove tonight's full desktop workflow.
- **Fix:** Rehearse current native build with the intended league/seat and fresh data; use an isolated mock/replay to exercise updates, manual pick/undo, reconnect, and chat. Then automate deterministic native coverage for those interactions.
- **Effort:** M
- **Grade lift:** B → B+ (covers the desktop boundary and full workflow).

## E — Security — B+

Chat CLI tools are disabled and child processes killed on cancellation/timeout (`src-tauri/src/chat_cli.rs:235`). Credentials have Keychain support, and companion routes require pairing/token checks (`src-tauri/src/companion/hub.rs`). Desktop CSP allows broad network connections for arbitrary companion addresses; root README documents this intentional tradeoff. No exploit was established and a transport redesign is not a tonight blocker. Audit exceptions are stated in validation rather than treated as a clean vulnerability count.

## F — Dependencies & Tech Currency — B+

Both npm trees and Cargo have committed locks; the Rust toolchain is pinned and updater/native test features are separated (`src-tauri/Cargo.toml`, `rust-toolchain.toml`). Live npm registry inspection found newer Playwright 1.63.0, Node types 26.5.0 and typescript-eslint 8.70.0, while TypeScript is pinned at 5.9.3. These are inventory observations, not recommendations for last-minute upgrades. Security policy passes with the documented exceptions. No dependency change is required by this audit.

## G — Performance & Scalability — B−

Production output is split into draft, season and chat chunks (`vite.config.ts`, build output); the largest main chunk is about 282 kB, 89 kB gzip. Poll view construction runs outside the main locks. An optional endpoint can nevertheless delay the latency-critical pick update path.

#### G1 — Keep member lookup retries from delaying picks
- **Where:** `draft-assistant/src-tauri/src/commands_draft/tick.rs:156`, `draft-assistant/src-tauri/src/sleeper/endpoints.rs:74`, `draft-assistant/src-tauri/src/sleeper.rs:263`, `draft-assistant/src-tauri/src/sleeper.rs:384`.
- **What's wrong:** When initial member lookup failed, the next ticks join successful picks with a full member lookup retry sequence. Three eight-second timeouts plus backoff can delay applying picks about 24.75 seconds, before the normal polling interval.
- **Impact:** Major — a board can lag during a short pick clock even when the picks endpoint works.
- **Fix:** Retry member resolution independently of picks (preferred), or bound its contribution with a short one-attempt request. Add a hanging-member-endpoint test proving successful picks are applied promptly.
- **Effort:** S
- **Grade lift:** B− → B (removes optional network work from the critical update delay).

## H — Documentation & Onboarding — B

README explains installation, refresh, manual picks, exports and preview limitations; companion API is separately documented. Some statements retained from older architecture are now inaccurate.

#### H1 — Correct obsolete server/build descriptions
- **Where:** `draft-assistant/README.md:11`, `draft-assistant/TESTING.md:135`, `draft-assistant/src-tauri/Cargo.toml:73`.
- **What's wrong:** “No server anywhere” and older Axum-free default-build expectations conflict with the embedded companion server and nonoptional Axum dependency.
- **Impact:** Minor — onboarding and verification instructions misdescribe the current architecture.
- **Fix:** Describe the optional embedded LAN server and distinguish legitimate companion dependencies from WDIO-only automation dependencies.
- **Effort:** S
- **Grade lift:** B → B+ (aligns documentation with the shipped architecture).

## I — Developer Experience & Tooling — B+

`npm run verify` provides strict formatting, lint, type, test and coverage gates; guard-script tests validate release and consistency tooling (`package.json`, `scripts/`). Native automation uses its own build directory. Failed browser runs currently lose useful CI evidence.

#### I1 — Retain browser failure artifacts in CI
- **Where:** `draft-assistant/playwright.config.ts:40`, `.github/workflows/ci.yml:117`.
- **What's wrong:** CI uses a list reporter and writes screenshots to `e2e-browser/.results`, but uploads `playwright-report`. Traces are configured for first retry while retries are zero.
- **Impact:** Moderate — browser failures lack the screenshots/traces needed for fast diagnosis.
- **Fix:** Add HTML reporting in CI, use retain-on-failure traces, and upload both report and results directories.
- **Effort:** S
- **Grade lift:** B+ → A− (makes failed test evidence available).

## Final validation addendum

- `npm run verify` exited 0: 1,408 Rust tests passed, none ignored in the reported test summaries. Rust coverage: 93.51% lines, 90.68% functions, above both configured floors. Production build passed.
- `npm run test:e2e` exited 0: one native WKWebView smoke test passed. It restored the current league shell on the Season screen; this is not a full draft rehearsal. The season screen reported no week-1 matchup rows from Sleeper, relevant to season use rather than pre-draft board support.
- The harness logged an external `tauri-driver` diagnostic and optional JS-injection warnings. Source inspection of installed `@wdio/tauri-service` showed diagnostics omit the configured embedded provider; the actual embedded native test nevertheless ran and passed. No driver installation or app-security relaxation was needed.
- C1 was experimentally reproduced with one temporary test; that intentionally failing audit probe was removed.
- Fresh regular **0.3.1** app built successfully with `npm run tauri build -- --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`; exit 0. `codesign --verify --deep --strict` passed. It is ad-hoc signed, not notarized. The first invocation built the app but exited 1 at updater signing because no release private key was supplied; the local-only override omitted updater artifact creation without editing repository configuration.
- The fresh app was opened and visibly restored the league, then switched to Draft. The live draft screen rendered recommendations, a 453-player board, live sync, and the explicit message “The draft order has not been posted yet.” Public Sleeper lookup independently confirms the configured user is a league member but has no assigned draft-order seat yet.
- Native chat settings detected Claude Code and offered both Opus 5 and Fable 5, with Claude Code/API key routes. A real in-app Opus 5 / Low / Claude Code turn returned **Draft chat connection OK** and was saved as a test conversation. The regular app is left open on Draft with chat visible. Fable and the separate API-key route were not called.
- Current-head hosted native End-to-end workflow completed successfully; hosted CI was still in progress at the latest inspection. Local canonical verification already passed.
