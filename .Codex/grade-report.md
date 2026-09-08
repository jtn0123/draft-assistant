# Codebase Grade Report — draft-night readiness

**Project:** Draft Assistant 0.3.1
**Audited:** 2026-09-07
**Source:** local working tree based on `d7a0bfe`, including the authorized remediation below. Not committed or pushed.
**Stack:** Tauri 2, Rust/Tokio, React 19, TypeScript, Vite.

This fresh audit replaces the August 27 grade report with explicit user approval. The user confirmed tonight’s platform is Sleeper and the requested Claude/Astra/Sol capability concerns Draft Assistant’s built-in chat. Findings below describe the audited source, not the old grade or tracker assertions. The original audit below is retained as the before-state; the remediation addendum records the subsequently authorized implementation and validation.

## September 7 remediation — current working tree

The user authorized all seven findings and requested removing the budget. This is local, uncommitted work based on `d7a0bfe`; hosted checks for the old commit do not validate these edits.

| ID | Implemented change | Evidence |
|---|---|---|
| B1 | Draft equality includes order/settings; pick signatures include metadata, keeper and slot changes. | Poll decision regressions; draft command tests. |
| G1 | Optional member lookup uses one request with a three-second bound, instead of full retries delaying picks roughly 25 seconds. | Failing and hanging member endpoint regressions. |
| C1 | New/Fresh/Carry actions are guarded while sending. | Deferred-answer tests, including choice bar opened before Send; disabled New observed natively. |
| C2 | Added GPT-6 Astra and GPT-5.6 Sol via the existing ChatGPT-authenticated Codex CLI; retained Claude Code/API routes. | Six transport tests; actual native Astra, Sol and Claude Opus turns passed. |
| D1 | Added a separately identified, fixture-backed WKWebView rehearsal for setup, advancing picks, manual pick/undo and outage recovery. | Native WKWebView scenario passed in 12.9 seconds; six screenshots retained. |
| H1 | Corrected embedded companion server/build documentation; documented AI authentication and estimates. | README, TESTING and companion API review. |
| I1 | CI retains HTML, screenshots and traces on browser failure, even with retries disabled. | Playwright/workflow configuration review; 38 production browser tests passed. |

**Cost-only chat:** removed budget fields, cap warnings and spending rejection from desktop and companion paths. Legacy budget commands/config fields remain harmless compatibility shims returning zero; saved limits cannot block answers. Both subscription routes now report API-equivalent usage estimates, explicitly described as estimates rather than extra token bills. Existing historical subscription turns that were recorded as zero are not retroactively repriced. Provider usage is required for a complete estimate; cancelled CLI turns may not return usage.

**Native AI validation:** the regular release app displayed all four model buttons without overflow, no budget input, and the running estimated cost. Actual low-effort replies were `Draft Astra native OK.`, `Draft Sol native OK.`, and `Draft Claude native OK.`; displayed cumulative estimate reached $0.14. No authentication tokens were read or copied into the app. Codex runs in a private temporary directory, ignores user/project config, disables tools/plugins/hooks and uses existing CLI authentication. This is subscription-backed Codex chat, not a newly implemented OpenAI API-key route. The separate Anthropic API-key path and Fable were not live-called.

### Final grade and readiness

**Overall: B+ (up from today's fresh B audit; the August 27 C+ is obsolete).** All seven addressable audit items are implemented and verified locally.

| Category | Final grade |
|---|---|
| Architecture | B+ |
| Backend | B+ |
| Frontend | B+ |
| Testing | B+ |
| Security | B+ |
| Dependencies | B+ |
| Performance | B |
| Documentation | B+ |
| DevEx | A− |

**Go for tonight as a Sleeper draft helper.** Continue submitting actual picks in Sleeper. The regular 0.3.1 native app is rebuilt and checked, and all three requested AI routes answered in its built-in chat. This is not proof of an uninterrupted entire live draft or the season workflow. The remaining live dependency is external: Sleeper still reports no draft-order seats. Once the commissioner publishes the order, verify that My Roster and your upcoming turns identify you correctly. The latest public API check still reports `pre_draft`, snake, zero seats, and **September 7 at 8:00:51 PM PDT (7:00:51 PM fixed PST)**. No schedule or real league picks were changed.

### Final validation evidence

- `npm run verify`: exit 0. 939 frontend tests in 81 files, 1,417 Rust tests, and 48 script tests passed. Frontend coverage 95.62% lines / 89.37% branches; Rust coverage 93.49% lines / 90.62% functions. Production build passed. The lower frontend count versus the original audit reflects removing obsolete cap tests, with new chat race/provider regressions added.
- Final `npm run verify:fast`: exit 0 after the new rehearsal files and preview contract updates; LOC, CSS, text, formatting, ESLint, Rust clippy and TypeScript passed. Final rehearsal selector corrections were separately formatted and typechecked.
- Production Chromium suite: all 38 tests passed without retries. The later preview-only cleanup changed legacy cap values to zero and matched the backend effort lists; TypeScript passed, and direct browser measurement confirmed no overflow of the four model buttons in the 380px panel.
- Native rehearsal build succeeded with opt-in WDIO and the separate `com.justin.draft-assistant.rehearsal` identifier. After correcting harness screen selection and selectors, the scenario passed in 12.9 seconds against that binary: setup, incoming API pick, manual pick, undo, HTTP 503 sync retry and recovery to Live with the next pick. This uses real IPC and WKWebView with local fixture data, no real league changes. The temporary profile link was removed; logs, fixture requests, isolated data and six screenshots remain at `/var/folders/h2/d10fqbgx4sg3xt10y73ynf_m0000gn/T/draft-native-rehearsal-Meq7Jl`. Final recovered screenshot was inspected. A supported explicit WebDriver window selection removes the service's repeated optional five-second focus probes.
- Final regular bundle build: exit 0 using the local updater-artifact override, version 0.3.1. `codesign --verify --deep --strict` passed. Bundle: `draft-assistant/src-tauri/target/release/bundle/macos/Draft Assistant.app`. It is locally ad-hoc signed, not a notarized/published release.
- Targeted Codex process/parser/cancel/timeout tests passed; targeted companion/command tests passed, including ignored legacy caps. Native Astra, Sol and Claude replies and estimated usage are described above.
- Existing dependency locks were not upgraded. The earlier same-lock audit passed its configured policy with documented exceptions; no claim of zero advisories. Hosted CI was not run for these unpushed edits.
- Final regular bundle reopened successfully, restored Sharks League on Draft with Live status, and retained the cost tally. Left a fresh GPT-5.6 Sol / Low chat open; saved connection-test history is preserved.
- `git diff --check` passed. No commit, push, merge or deployment was performed.

## Follow-up: settings, identity, scrolling and remote setup

Implemented the subsequently requested usability work:

- Gear menu now uses a toothed settings icon and keeps only quick draft controls. All settings opens a dedicated page grouped into Draft identity, Remote connections, Appearance & sound, Draft data, and Diagnostics & updates. The underlying draft stays mounted; returning preserves board filters and polling.
- Sleeper identity includes manual entry and selectable current-league account names, current default and draft-seat status. The league endpoint supplies display names rather than a separate username field; saving uses stable account IDs, never custom fantasy-team names. The native app loaded all 12 current accounts and successfully re-saved the existing identity without changing the selected person.
- Player rows scroll independently from roster/sidebar/chat, with pinned table headings and fixed filters. Native scrolling was visually verified; browser measurement confirmed the page and chat stayed still.
- Model picker starts collapsed, expands on demand and collapses after selection, returning keyboard focus to the selected-model button. Verified in native UI with GPT-5.6 Sol.
- Fixed HTTP 401 follower revocation leaving its socket active. The socket now stops and ignores late connection events, frames and pending refreshed snapshots. Remote draft actions now visibly say Controlled by host and stay disabled; server/API authorization remains enforced.
- Both remote client flows passed against a real isolated Rust HTTP/WebSocket server: actual phone-page pairing, desktop-frontend pairing, write restrictions, shared chat-reset permission, simultaneous offline/reconnect with newer fixture state, and revoke-all. The final remote run passed in 6.22 seconds and verified disabled draft controls before and after recovery. Six screenshots are in `/var/folders/h2/d10fqbgx4sg3xt10y73ynf_m0000gn/T/draft-assistant-companion-browser-rehearsal-58071-1788826380134143000/browser-evidence`. These were separate Chromium contexts over loopback, not a physical phone and second computer on Wi-Fi.
- Final `npm run verify` exited 0: 948 frontend tests in 83 files, 1,419 Rust tests and 48 script tests passed. One opt-in remote browser test was excluded from the default Rust run and explicitly passed separately as described above. Frontend coverage 95.60% lines / 89.06% branches; Rust 93.36% lines / 90.62% functions. All 39 production browser tests passed. Final `npm run test:e2e:rehearsal` exited 0: one native WKWebView scenario passed in 13 seconds, including incoming picks, manual pick, undo, outage/recovery with the new scroll wrapper. Final native evidence: `/var/folders/h2/d10fqbgx4sg3xt10y73ynf_m0000gn/T/draft-native-rehearsal-GicKU4`; test profile cleanup confirmed.
- The user clarified physical clients are on another network. Existing Tailscale/MagicDNS/optional HTTPS support is implemented, but live local inspection found no installed Tailscale executable/application or active tailnet-range interface. Cross-network hardware connectivity remains unverified until the devices have a private route. Tailscale 1.102.3 was subsequently installed from its Apple-notarized, Tailscale-signed standalone package; onboarding reached account sign-in. The Mac subsequently reached Tailscale Running/online and companion hosting was enabled on port 7878. The user initially reported an unchanged Connect button in the iPhone Camera QR browser; after the direct Safari retry instructions, the user confirmed it was working and began testing two phones. The user then confirmed the two-phone setup fully works. This is physical-device success reported by the user, distinct from automated local evidence. Current scripts are served correctly; Chromium, WebKit and simulated iPhone WebKit on the LAN all show the expected invalid-code error. The exact cause of the Camera-browser failure remains unconfirmed. Added a startup loading/error message, disabled Connect until successful startup, and JavaScript-disabled guidance; 60 companion page tests passed. The host was left running during the phone test; the later mobile-polish build was subsequently loaded and live asset bytes verified. No public exposure was configured. Corrected obsolete same-Wi-Fi-only/budget wording and added private-network setup guidance in both host/join dialogs; all 27 focused remote-dialog tests passed after this copy-only follow-up. The final local release build and strict code-signature verification also passed.
- The final regular 0.3.1 bundle built successfully and strict code-signature verification passed. Reopened the final app on Settings with all 12 account choices, the existing default preserved and polling live behind the page. No commit, push or deployment was requested or performed.

## Mobile polish follow-up

- After the user confirmed the two-phone setup fully works, refined the companion's pairing screen, draft heading, highlighted recommendation, roster, chat bubbles and touch-sized icon navigation. Kept system light/dark appearance and safe-area spacing.
- Added suggested questions that fill and focus the composer without sending, clear pairing progress/retry messages, and startup failure guidance. Clarified next-turn availability odds and waiting/paused/completed draft states.
- 73 companion tests passed, with focused lint/format checks. A separate current-source browser preview passed WebKit at 320px light/390px dark and Chromium at 430px light: pairing, tab navigation, hidden season-only tab, no horizontal overflow, suggestion focus, and composer clearance at reduced viewport height. Screenshots are in `/tmp/draft-mobile-polish`; fixture data only. The final real Rust host phone/desktop pairing, permissions, outage recovery and revoke rehearsal passed (1/1, 10.45 seconds). The local release build and strict signature check passed, the native host was restarted, and all five changed served mobile assets matched current source byte-for-byte. No commit or push.

## Mobile feature parity follow-up

- Added a prominent live countdown and remaining-time bar, 15-second warning styling, per-pick duration before starting, and explicit waiting/paused/completed handling.
- Added upcoming managers, picks until your turn, next personal pick numbers, and an expandable 24-pick queue. Queue ownership uses the desktop plain-snake baseline plus backend trade/third-round-reversal overrides and skips keeper picks. Expansion survives clock ticks and draft updates.
- Added device-local collapsible model/effort selection. An authenticated, credential-free host catalog advertises Claude Opus/Fable and GPT-6 Astra/GPT-5.6 Sol availability; validated POST fields reach the existing answer path without modifying the host provider or credentials. Older clients retain defaults.
- Combined companion frontend suite: 80 tests passed; TypeScript and file-length gate passed. Backend lane: 16 targeted tests, Clippy and formatting passed. Current-source WebKit/Chromium mobile previews verify all tabs, model selection/collapse, no horizontal overflow and composer clearance at reduced height. The final isolated real-host phone/desktop pairing and reconnect rehearsal passed (1/1, 8.27 seconds). Release build and strict signature verification passed; the native host was restarted and all six relevant served assets matched source. The new model catalog correctly returned HTTP 401 without pairing. Phone refresh loads the new features; current Sleeper draft order is still pending.

## Original audit summary

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

## Original audit verdict (superseded by remediation below)

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
- **Where:** `draft-assistant/e2e/specs/launch.e2e.ts:25`, `draft-assistant/playwright.config.ts:4`.
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

## Pre-remediation validation

- `npm run verify` exited 0: 1,408 Rust tests passed, none ignored in the reported test summaries. Rust coverage: 93.51% lines, 90.68% functions, above both configured floors. Production build passed.
- `npm run test:e2e` exited 0: one native WKWebView smoke test passed. It restored the current league shell on the Season screen; this is not a full draft rehearsal. The season screen reported no week-1 matchup rows from Sleeper, relevant to season use rather than pre-draft board support.
- The harness logged an external `tauri-driver` diagnostic and optional JS-injection warnings. Source inspection of installed `@wdio/tauri-service` showed diagnostics omit the configured embedded provider; the actual embedded native test nevertheless ran and passed. No driver installation or app-security relaxation was needed.
- C1 was experimentally reproduced with one temporary test; that intentionally failing audit probe was removed.
- Fresh regular **0.3.1** app built successfully with `npm run tauri build -- --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`; exit 0. `codesign --verify --deep --strict` passed. It is ad-hoc signed, not notarized. The first invocation built the app but exited 1 at updater signing because no release private key was supplied; the local-only override omitted updater artifact creation without editing repository configuration.
- The fresh app was opened and visibly restored the league, then switched to Draft. The live draft screen rendered recommendations, a 453-player board, live sync, and the explicit message “The draft order has not been posted yet.” Public Sleeper lookup independently confirms the configured user is a league member but has no assigned draft-order seat yet.
- Native chat settings detected Claude Code and offered both Opus 5 and Fable 5, with Claude Code/API key routes. A real in-app Opus 5 / Low / Claude Code turn returned **Draft chat connection OK** and was saved as a test conversation. The regular app is left open on Draft with chat visible. Fable and the separate API-key route were not called.
- Current-head hosted native End-to-end workflow completed successfully; hosted CI was still in progress at the latest inspection. Local canonical verification already passed.
