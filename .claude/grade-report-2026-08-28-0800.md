# Codebase Grade Report

**Project:** draft-assistant
**Audited:** 2026-08-28, 08:00–08:30 PDT — **draft day, ~8.5 hours to first pick**
(the morning report is archived at `.claude/grade-report-2026-08-28-morning.md`; the 2026-08-27 one at `.claude/grade-report-2026-08-27.md`. Item IDs below start fresh.)
**Stack:** Tauri 2 desktop app — Rust/Tokio core (`src-tauri/`, ~4,200 LOC incl. tests) + React 19 / TypeScript-strict / Vite 7 frontend (`src/`, ~1,900 LOC TS/TSX + ~930 CSS), Bun; ~9,600 LOC first-party

## Summary

| ID | Category | Grade | Items |
|----|----------|-------|-------|
| A | Architecture & Design | B+ | 3 |
| B | Backend Quality | B+ | 5 |
| C | Frontend Quality | B+ | 5 |
| D | Testing & Reliability | B | 5 |
| E | Security | B | 4 |
| F | Dependencies & Tech Currency | B− | 4 |
| G | Performance & Scalability | B+ | 3 |
| H | Documentation & Onboarding | B+ | 3 |
| I | Developer Experience & Tooling | B+ | 4 |
| **Overall** | | **B+** | **36** |

**Top 5 highest-leverage fixes:** D1, B1, A2, D2, C1

---

## Draft-day readiness — the answer to "is it ready for tonight"

**Verdict: yes — ship it. Green with three yellows, none of them code.**

| | Status | Evidence |
|---|---|---|
| **Correctness on your actual league** | 🟢 | Tonight's real state dumped and rendered end to end at 08:35: pick 1, round 1, `picks_until_mine` 1, your picks `2 · 27 · 30 · 55`, your keepers at 139/195 excluded from that list and present on your roster (R10/R14), open starters recomputed around them, 394 of 419 players available, **zero warnings** |
| **The desktop app runs today's code** | 🟢 | Launched `bun run tauri dev` at 08:05; process ran clean, no panic, and it rewrote `config.json` at 08:05 — proving the whole round trip (React → Tauri IPC → engine → Sleeper → view). *Caveat: this machine denies screen recording, so the window contents were not seen — see YELLOW 3* |
| **Live sync under load** | 🟢 | 5-minute soak over 18 live picks: heap flat at 16 MB, DOM stable at ~3,208 nodes, zero console errors, sort/filter/scroll all survive updates |
| **Failure behaviour** | 🟢 | Malformed state → error-boundary screen, not a white screen. Every cache falls back to its stale copy with an age-stamped banner. Poll failures colour the pill; 30 s of silence now reads "Sync stale". Corrupt saved settings fall back to defaults |
| **Manual fallback if Sleeper's API lags** | 🟢 | Keeper-correct as of today: a manual pick takes the next *open* number and is no longer discarded by a keeper sitting at pick 177 (this was silently broken this morning) |
| **Ask Claude** | 🟢 | CLI present (`claude` 2.1.250) and previously smoke-tested live end to end (36 s, full board in context) |
| **Caches** | 🟡 | Written 03:13 today. Players TTL 24 h → fine. **Projections TTL is 6 h → they expire ~09:13**, so tonight's first launch re-fetches ~20 MB of projections under an 8-second total-transfer cap (B1). On venue wifi that can trip and fall back to stale with a banner — harmless but noisy. **Click Refresh data on good wifi before you leave** |
| **Off-machine backup** | 🟡 | 28 commits unpushed; `.github/workflows/verify.yml` has therefore never run today's code. Locally `bun run verify` is green (105 Rust, 58 Vitest, 15 Playwright, clippy, eslint, LOC cap) |
| **Eyes on the real window** | 🟡 | The app was launched and its backend verified, but the window was never *seen* (no Screen Recording permission for this session). Spend 60 seconds before 17:00 looking at it yourself |

**Residual risk, ranked:** (1) venue wifi during the 17:00 re-fetch — mitigated by refreshing beforehand and by stale-cache fallback; (2) the Tauri command layer (`desktop.rs`, 354 lines) still has no automated tests, so an IPC-layer regression would only be caught by launching the app — which is why launching it once tonight matters; (3) nothing else known.

**Do not execute any item below before the draft.** Every one touches code that is currently verified green. The only pre-draft actions worth taking are operational: refresh data on good wifi, launch the app once and look at it, and push the branch.

---

## A — Architecture & Design — B+

The dependency graph is still strictly one-way (`sleeper` → `scoring`/`roster`/`valuation` → `board`/`draft` → `recommend` → `view` → `engine`/`simulation` → `desktop`/`chat`) and today added three genuinely good seams: `manual.rs` (pure apply/undo over `LoadedLeague`), `view::next_open_pick`/`poll_fingerprint` (pure functions the poll loop and the manual path both consume), and `draft::DraftOrder` (snake/linear/third-round-reversal behind one type, `draft.rs:33-77`). The chat backend is three focused modules (`chat/mod.rs`, `prompt.rs`, `cli.rs`) instead of one file. Every first-party file is ≤500 lines, enforced in `verify` by `scripts/check-loc.mjs`. What holds it at B+ rather than A−: the TypeScript contract is still transcribed by hand, and the app's most important runtime behaviour — the poll loop — is still 90 lines inlined in a Tauri command with no seam to test.

#### A1 — Generate `types.ts` from the Rust structs instead of mirroring by hand
- **Where:** `src/types.ts:1-188`; sources in `src-tauri/src/view.rs:19-110`, `board.rs:12-39`, `draft.rs:79-96`, `recommend.rs:9-23`
- **What's wrong:** Twelve interfaces are transcribed by hand, and today's schema 1.2 → 1.3 bump proved the cost: `start_time` and `pick_deadline` had to be added in lockstep to `view.rs`, `types.ts`, `api.ts` and `public/dev-fixture.json`, and the only thing that would have caught a miss is the `schema_version` string compare (`api.ts:12-21`), which catches a forgotten bump but not a drift.
- **Fix:** Add `ts-rs` as a dev-dependency, derive `TS` on the view structs, emit `src/types.generated.ts` from `cargo test`, and have `verify` fail on a diff. Keep `schema_version` as the runtime guard.
- **Effort:** M
- **Grade lift:** B+ → A− (the Rust↔TS boundary becomes compiler-checked)

#### A2 — Extract the poll loop from `desktop.rs`
- **Where:** `src-tauri/src/desktop.rs:221-313` (the command), `:238-311` (the loop body)
- **What's wrong:** `start_polling` is a 90-line command whose body is the entire live-sync state machine — fetch, fingerprint, manual-pick reconciliation, health accounting, two emits — with no unit-testable boundary. D1 below is the direct consequence: the code most likely to fail during a draft is the code with no tests. Today's fingerprint fix had to be tested through a free function precisely because the loop itself cannot be.
- **Fix:** Move the body into `poll.rs` as `async fn poll_once(engine, loaded, cursor) -> PollOutcome` returning `{ changed, health, errors }`; leave the command to spawn, sleep and emit.
- **Effort:** S
- **Grade lift:** B+ → A− (the most important runtime behaviour gets a testable seam)

#### A3 — Delete the `view_from` alias and the `chat` command/module name clash
- **Where:** `src-tauri/src/desktop.rs:25-27` (`view_from`), `:7`, `:192`, `:209` (`crate::chat::ask` disambiguation comment)
- **What's wrong:** `view_from` is a one-line pass-through to `build_view` with no added behaviour, used in nine places. The `chat` command shadows the `chat` module, forcing a fully-qualified call and a comment explaining why.
- **Fix:** Delete `view_from`; rename the command to `ask_claude` and update the `api.ts:52` invoke string.
- **Effort:** S
- **Grade lift:** B+ → B+ (readability only; no structural change)

---

## B — Backend Quality — B+

The model layer had a good day. Survival odds are now conditioned on the player still being available (`draft.rs:126-148`) instead of reading 1 % for every faller; tiers split when a band spans more than 1.5× the gap (`valuation.rs:108-129`); the ADP column follows league scoring and roster shape (`board.rs:41-56`); draft type and third-round reversal are honoured (`draft.rs:33-96`) with a warning for auction; and the keeper model — `next_open_pick`, number-keyed `merged_picks`, keeper-aware `apply_manual_pick` — closed two defects that would have hit live tonight (`view.rs:130-160`, `manual.rs:14-40`). Every request now carries the shared 3 s/8 s bounds including the username lookup (`sleeper.rs:203-219`, `:240-249`), and errors carry their cause chain (`sleeper.rs:376-390`). What keeps it at B+: one timeout is wrong for two specific requests, there is still no log anyone can read, and one silent-wrong-team path survives.

#### ~~B1 — Give the two large downloads their own timeout~~ ✓ done 2026-08-28 (`f1339b0`; 60 s cap on `players` and `weekly_projections`, tested against a silent server)
- **Where:** `src-tauri/src/sleeper.rs:211-212` (blanket 8 s), `:280` (`players`, ~14 MB raw), `:294-306` (`weekly_projections`, 18 MB on disk); callers `engine.rs:94-123`, `:158-203`
- **What's wrong:** reqwest's `timeout` is total transfer time, not idle time, and one 8-second cap covers every request including the two biggest. It has never tripped here (cold load measured ~6 s on home wifi today) but venue wifi is the scenario it exists for. Projections carry a 6-hour TTL (`engine.rs:18`) and were last written at 03:13, so tonight's first launch re-fetches them.
- **Fix:** Keep the client default; add `.timeout(Duration::from_secs(60))` on the `players` and `weekly_projections` request builders only. Add one test that the stale-cache fallback engages on timeout.
- **Effort:** S
- **Grade lift:** B+ → A− (the largest transfers stop being the ones most likely to trip the cap)

#### B2 — Write a log file the user can actually find
- **Where:** `src-tauri/src/engine.rs:182` (the crate's only `eprintln!`), `desktop.rs:301`, `:308` (`.ok()` discards emit failures), `:318-353` (no log plugin registered)
- **What's wrong:** A packaged `.app` sends stderr nowhere. Poll failures exist only as a pill colour, cache fallbacks only as a banner, and a failed `draft-updated` emit is dropped without trace. If the board freezes at pick 40 tonight there will be nothing to read afterwards.
- **Fix:** Register `tauri-plugin-log` writing to `~/Library/Logs/com.justin.draft-assistant/`. Log at info: each emit with `seq` and pick count, cache hit/fallback with age, chat spawn/exit/duration; at warn: every poll error and emit failure. ~10 call sites.
- **Effort:** S
- **Grade lift:** B+ → A− (post-mortems become possible)

#### B3 — Gate the mock-draft slot fallback on an explicit flag
- **Where:** `src-tauri/src/view.rs:210-227` (`user_names.is_empty()` at `:214`); `engine.rs:232-257`
- **What's wrong:** Carried from both prior reports. The fallback that adopts the draft creator's slot as "yours" fires whenever `user_names` is empty — which a transient failure of `/league/{id}/users` in a real league also produces. Reachable only when `my_user_id` is unresolved too, which is not your config, so it stays mid-list.
- **Fix:** Add `is_mock: bool` to `LoadedLeague`, set only in `load_draft_only`, and gate the fallback on it; push a warning when the users fetch fails.
- **Effort:** S
- **Grade lift:** B+ → A− (a silently-wrong-team state becomes impossible in a real league)

#### B4 — Percent-encode the username path segment
- **Where:** `src-tauri/src/sleeper.rs:246` (`format!("user/{username}")`), called from `desktop.rs:59-70` and `bin/dump_state.rs:57-61`
- **What's wrong:** Whatever is typed on the Setup screen is interpolated raw into the URL path. A username containing `/`, `?` or `#` silently requests a different endpoint. Low impact — the user is attacking only themselves on a read-only public API — but it is the one unvalidated string that reaches a URL.
- **Fix:** Percent-encode the segment (or reject anything outside `[A-Za-z0-9_-]` with a clear message, which Sleeper's own rules allow).
- **Effort:** S
- **Grade lift:** B+ → B+ (correctness nit; no realistic exploit)

#### B5 — Surface `is_keeper` in the view
- **Where:** `src-tauri/src/sleeper.rs:82-96` (`Pick` has no `is_keeper` field though the API sends it), `view.rs:46-54` (`RecentPick`), `draft.rs:79-96` (`RosterEntry`)
- **What's wrong:** The app now *handles* keepers correctly but never *says* "keeper" anywhere. Your two keepers show on your roster as ordinary R10/R14 picks, and other teams' keepers are simply absent from the board with no explanation. In a keeper league that is a missing label on the single most distinctive fact about the draft.
- **Fix:** Deserialize `is_keeper`, carry it through `RosterEntry` and `RecentPick`, and render a small "keeper" tag in the roster list and in recent picks once the draft reaches those numbers.
- **Effort:** S
- **Grade lift:** B+ → A− (the data model stops hiding a fact the UI should show)

---

## C — Frontend Quality — B+

Two dogfood passes today closed seventeen issues in this layer, and the second pass found only four. The app now never claims success for an action that failed (`App.tsx:96-108`, `:167-190`), renders launch failures above the Setup screen instead of dropping them (`App.tsx:200-224`), counts the sync age on a timer and calls a silent feed stale (`App.tsx:120-125`, `:363-386`), and scopes its live region to the status text so a screen reader is not read the countdown once a second (`Panels.tsx:96-118`). Accessibility now checks out on inspection: `scope="col"` on all eleven headers plus a caption (`Board.tsx:143-176`), h1→h2 heading order, labelled controls, visible focus on every tab stop, and AA contrast everywhere measured. What is left is scope, not correctness: no way back to Setup, a hard 200-row cap, and a tier colour scale that ran out of colours when tiers started running to 17.

#### C1 — Add a way back to Setup / a league switcher
- **Where:** `src/App.tsx:200-224` (Setup renders only when `view === null`); `src/components/Panels.tsx:9-64` (Setup); `AppConfig.leagues` is populated (`engine.rs:30-43`) and never read by the UI
- **What's wrong:** Carried from both prior reports. Once a league loads there is no path back: no league switcher, no "change username", no way to correct a mistyped league ID short of deleting `config.json` by hand. The config already stores a list of leagues that nothing renders.
- **Fix:** Add a small league menu in the header listing `config.leagues` plus "Add another league…" which reopens Setup with the current values.
- **Effort:** M
- **Grade lift:** B+ → A− (removes the only dead end in the app)

#### ~~C2 — Give the 200-row cap an escape hatch~~ ✓ done 2026-08-28 (`82068f8`; "Show all N" / "Show top 200")
- **Where:** `src/components/Board.tsx:107` (`matching.slice(0, 200)`), `:134-139` (the count)
- **What's wrong:** With 394 players available, "Showing 200 of 394" is the only sign that a third of the board is unreachable except by search. Scrolling to the bottom just stops. Sorting by ADP descending or Bye ascending — both plausible mid-draft — silently hides the tail.
- **Fix:** Render a "Show all 394" button in the count slot that lifts the cap for the session; keep the cap as the default.
- **Effort:** S
- **Grade lift:** B+ → A− (no data is unreachable without knowing to search)

#### C3 — Extend the tier colour scale past five
- **Where:** `src/components/Board.tsx:205` (`tier-${Math.min(p.tier, 5)}`), `src/components.css:289-293` (five classes)
- **What's wrong:** Today's tier banding pushed numbers to T17, so every badge from T5 down renders in the same colour — ten distinct tiers, one colour. The alert wording was fixed this morning ("Top tier T7"); the badges were not.
- **Fix:** Add `tier-6`, `tier-7`, `tier-8` steps and clamp at 8, or switch to three semantic buckets (top/mid/deep) driven by tier relative to that position's tier count.
- **Effort:** S
- **Grade lift:** B+ → A− (the tier column carries signal again below the top four bands)

#### C4 — Show the draft's own progress, not just yours
- **Where:** `src/components/Panels.tsx:120-160` (banner), `src/App.tsx:285-300` (main grid); `view.rosters` is populated for all 14 teams and rendered nowhere
- **What's wrong:** The view carries every team's roster and the UI shows only yours. In a keeper league that means you cannot see who kept whom without leaving for Sleeper — the exact context you need for "should I reach for a QB".
- **Fix:** Add a collapsed "All rosters" section under the side panel rendering `view.rosters` as 14 compact columns.
- **Effort:** M
- **Grade lift:** B+ → A− (closes the last reason to switch to Sleeper mid-draft)

#### ~~C5 — Focus the search box with a keystroke~~ ✓ done 2026-08-28 (`82068f8`; `/` focuses and selects, ignored while typing or with a dialog open)
- **Where:** `src/components/Board.tsx:127-133`; document-level key handling exists only in `Chat.tsx:44-56`
- **What's wrong:** The most common mid-draft action — find a player by name — needs a mouse. With 90 seconds on the clock that is the wrong default.
- **Fix:** Bind `/` (and `⌘F`) at the document level to focus the search input, ignoring the binding while a dialog or the chat input has focus.
- **Effort:** S
- **Grade lift:** B+ → B+ (speed, not capability)

---

## D — Testing & Reliability — B

The suite is real and it earned its keep today: 105 Rust tests (unit + 13 property + 6 keeper + 7 view-feed + 7 parsing + 2 simulation + fixture), 58 Vitest, 15 Playwright, three fuzz targets, and a replay harness (`scripts/replay-sleeper.mjs`) that stands in for Sleeper so a whole draft can be replayed against the real UI. Every one of today's 30 fixes was written test-first — the failing run is recorded in each dogfood report — and the property tests over `DraftOrder` now cover snake, linear and reversal. What holds it at B is unchanged from this morning and is the same sentence as last time: the two surfaces most likely to break during a live draft — the Tauri command layer and the engine's fetch/fallback path — are the two with almost no tests.

#### D1 — Test the Tauri command layer
- **Where:** `src-tauri/src/desktop.rs` (354 lines, **zero** `#[test]`); the commands at `:32`, `:59`, `:89`, `:126`, `:141`, `:160`, `:177`, `:192`, `:221`
- **What's wrong:** Every IPC entry point is untested: the save-then-roll-back on a failed manual pick (`:148-157`, `:164-173`), the lock ordering, the poll loop's change detection, the health accounting. `manual.rs` and `poll_fingerprint` were extracted precisely so their logic could be tested — the wiring around them still cannot be.
- **Fix:** Do A2 first, then test `poll_once` against a `SleeperClient` pointed at a local stub server (the base-URL seam already exists and the replay script proves it works): no-change → no emit, new pick → emit, fetch error → failure counter, same count different player → emit.
- **Effort:** M
- **Grade lift:** B → A− (the live-sync state machine gets covered)

#### D2 — Test the engine's fetch, cache and fallback path
- **Where:** `src-tauri/src/engine.rs:94-203` (two tests in the file, both about manual picks: `:396`)
- **What's wrong:** Cache-hit, cache-miss, fetch-failure-with-stale-fallback, fetch-failure-with-no-cache and the "could not be cached" warning are all reachable only by taking the network away by hand. This is the code that decides what the board is built from.
- **Fix:** Point `SleeperClient` at a stub server per case (200 with a fixture, 500, a hang) and assert the returned warnings and the age stamp. Six tests would cover the matrix.
- **Effort:** M
- **Grade lift:** B → A− (the data path's failure modes stop being untested)

#### ~~D3 — Run CI on this branch~~ ✓ done 2026-08-28 (PR #1; three real failures found and fixed, run 33188132961 green)
- **Where:** `.github/workflows/verify.yml` (triggers: push to `main`, pull_request); branch `t3code/review-prior-grade-report` is 28 commits ahead of its remote and 37 ahead of `origin/main`
- **What's wrong:** The workflow exists and has never executed a single line of today's code. Everything green is green on one machine, with one toolchain, in one worktree.
- **Fix:** Push the branch and open a PR (or add `push: branches: ['**']`). Confirm the macOS runner reproduces `bun run verify`.
- **Effort:** S
- **Grade lift:** B → B+ (green stops meaning "green here")

#### D4 — Add a smoke test that the desktop shell actually boots
- **Where:** no coverage; `src-tauri/src/desktop.rs:318-353` (`run()`), `src/main.tsx`
- **What's wrong:** Today the app was launched by hand and verified only indirectly (it rewrote `config.json`). Nothing automated would catch "the window opens white" — the failure mode that matters most and is currently invisible to the whole suite.
- **Fix:** Add a `cargo test --features desktop` smoke test that builds the `AppState`, calls `get_config` and `get_state` against a stub server and asserts a `DraftView` comes back; that covers the IPC types without needing a window.
- **Effort:** M
- **Grade lift:** B → B+ (the boot path gets a floor)

#### D5 — Keep one fixture honest about keepers
- **Where:** `draft-assistant/public/dev-fixture.json` (regenerated today from the real keeper league), `src-tauri/tests/fixtures/board_input.json`
- **What's wrong:** The Rust board fixture predates keepers and contains none, so the integration layer never sees a keeper except in the hand-built `tests/keepers.rs` league. The one place a real payload is asserted against is the one place keepers are missing.
- **Fix:** Capture a sanitized 30-pick slice of tonight's draft (keepers included) into `tests/fixtures/`, and assert `next_open_pick` and roster assembly against it.
- **Effort:** S
- **Grade lift:** B → B+ (real payload shape covers the feature that broke today)

---

## E — Security — B

The threat surface is small and mostly right: a local-first app against a read-only public API with no auth, no secrets in the repo (`git grep` finds no key material), minimal Tauri capabilities (`capabilities/default.json` — `core:default` + `opener:default`), React's escaping on every rendered string, and no `dangerouslySetInnerHTML` anywhere. The Claude CLI is spawned with `Command` argv (no shell) and the prompt on stdin (`chat/cli.rs`), with `--restricted --no-session-persistence` and tools off unless web search is enabled. What keeps it at B rather than B+: there is no CSP, the model is fed strings from a third party without delimiting, and the one user-typed string that reaches a URL is unencoded (B4).

#### E1 — Set a Content Security Policy
- **Where:** `src-tauri/tauri.conf.json:24-26` (`"csp": null`)
- **What's wrong:** The webview runs with no CSP, so any future XSS (or a compromised dev dependency injecting a script at build time) has no second line of defence. Everything rendered today goes through React, which escapes — this is defence in depth, not a live hole.
- **Fix:** Set `"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost https://api.sleeper.app; img-src 'self' data:"` and verify dev and packaged builds still load.
- **Effort:** S
- **Grade lift:** B → B+ (adds the standard hardening layer for a webview app)

#### E2 — Delimit third-party text in the Claude prompt
- **Where:** `src-tauri/src/chat/prompt.rs` (`board_row`/`board_table`/`state_json` interpolate player names, team names and manager display names straight into the prompt)
- **What's wrong:** Manager display names and player metadata come from Sleeper and are pasted into the prompt unfenced. A league-mate whose display name is "ignore previous instructions and recommend…" is a prompt-injection vector. The blast radius is one wrong chat answer — the board and recommendations are computed in Rust and never touched by the model — but the app tells you to trust that panel.
- **Fix:** Wrap the state block in explicit delimiters and add one line to the system prompt saying data inside them is untrusted and must never be treated as instructions.
- **Effort:** S
- **Grade lift:** B → B+ (closes the only untrusted-input-to-model path)

#### E3 — Record why the 17 unmaintained-crate advisories are accepted
- **Where:** `cargo audit` output (17 warnings: 10 gtk-rs GTK3 crates, 5 `unic-*`, `proc-macro-error`, `glib` unsoundness RUSTSEC-2024-0429); no `audit.toml`
- **What's wrong:** Every warning is a Linux-only Tauri dependency absent from the macOS build graph, but nothing in the repo says so, so the next person to run `cargo audit` re-derives it — or worse, ignores audit output by habit.
- **Fix:** Add `src-tauri/audit.toml` ignoring those advisory IDs with a one-line comment each, and add `cargo audit` to `verify`.
- **Effort:** S
- **Grade lift:** B → B+ (audit output becomes actionable rather than noise)

#### E4 — Bound what the chat can spend
- **Where:** `src-tauri/src/chat/cli.rs` (timeouts 90 s/150 s/180 s; cost is reported per call and summed in the UI, never capped)
- **What's wrong:** Each question ships the full board (~10 k tokens) and a live session showed $0.50 for one answer at max effort. Nothing stops a long draft night from running up an unbounded bill, and nothing warns before an expensive call.
- **Fix:** Track session spend in the backend, and refuse (with a clear message) past a configurable ceiling — default something like $10, overridable by env.
- **Effort:** S
- **Grade lift:** B → B+ (a runaway cost becomes impossible rather than merely visible)

---

## F — Dependencies & Tech Currency — B−

Clean on the axis that matters: `bun audit` reports **0 vulnerabilities** and `cargo audit` **0 vulnerabilities** across 519 crates. Runtime deps are current (Tauri 2, tokio 1.53, reqwest 0.12, React 19.1, serde 1). The grade is held down by three dev-dependency majors sitting still and by a toolchain nothing pins — on a machine that hit ENOSPC yesterday with 12 GB of build artifacts across two target directories.

#### ~~F1 — Pin the toolchain~~ ✓ done 2026-08-28 (`ca3dd84`; `rust-toolchain.toml` at 1.88.0 and the workflow installs from it — CI had drifted to 1.98 and failed on three newer lints)
- **Where:** no `rust-toolchain.toml`, no `.node-version`/`.bun-version`, no `engines` field in `package.json`; current: rustc 1.88.0, bun 1.3.14, node 26.7.0
- **What's wrong:** CI (`macos-latest` + `setup-bun@v2` with no version) and this machine can silently diverge, and a rustc bump can change lint behaviour under `-D warnings`, turning a green branch red for reasons unrelated to the change.
- **Fix:** Add `rust-toolchain.toml` (channel 1.88.0), pin Bun in the workflow, add `engines.node` to `package.json`.
- **Effort:** S
- **Grade lift:** B− → B (builds become reproducible)

#### F2 — Take the three dev-dependency majors
- **Where:** `package.json:35-52` — vite 7.3.6 → 8.2.2, typescript 5.8.3 → 7.0.2, @vitejs/plugin-react 4.7.0 → 6.1.1
- **What's wrong:** Three majors behind on the build toolchain. None ships in the app, so the risk is drift rather than exposure — but TypeScript 7 in particular changes inference in ways worth meeting deliberately rather than at the next `bun install`.
- **Fix:** After the draft: bump one at a time, run `bun run verify` between each, and read the TS 7 migration notes for the `strict` + `noUnusedLocals` combination this repo uses.
- **Effort:** M
- **Grade lift:** B− → B (build toolchain rejoins the supported line)

#### F3 — Enable Dependabot version updates, not just security ones
- **Where:** no `.github/dependabot.yml`; the only Actions run in the repo's history is a Dependabot *security* job
- **What's wrong:** Security updates arrive automatically; ordinary version updates never do, which is how three majors accumulated.
- **Fix:** Add `.github/dependabot.yml` covering `npm` and `cargo`, weekly, grouped by minor/patch.
- **Effort:** S
- **Grade lift:** B− → B (drift is surfaced continuously)

#### F4 — Reclaim the build artifacts
- **Where:** `src-tauri/target` (~7.6 GB) and `src-tauri/fuzz/target` (~4.3 GB) in this worktree
- **What's wrong:** Twelve gigabytes for one worktree on a disk that ran out of space during a prior session — and ENOSPC during a cache write is exactly the failure the store code now has to handle.
- **Fix:** `cargo clean` the fuzz target between campaigns; consider `CARGO_TARGET_DIR` shared across worktrees.
- **Effort:** S
- **Grade lift:** B− → B− (operational hygiene; no grade effect)

---

## G — Performance & Scalability — B+

Measured today, not estimated: 126 ms to interactive in the browser preview, a 5-minute soak holding 16 MB heap and ~3,208 DOM nodes flat across 18 live picks, smooth scrolling over 200 rows (~15 ms/frame), a 224 kB JS + 11 kB CSS bundle (628 kB `dist`), the 419-player board rebuilt on every view with no measurable cost, and a cold engine load of ~6 s including a 14 MB player dictionary. Caching is sensible (24 h players, 6 h projections, stale-fallback on failure). The remaining items are about the largest transfers and the row cap, not about hot loops.

#### G1 — Cache the board build instead of rebuilding per view
- **Where:** `src-tauri/src/view.rs:142-330` (`build_view` clones the whole available list on every call), called from every command and every 3 s poll (`desktop.rs:25-27`)
- **What's wrong:** Each `build_view` clones ~394 `BoardPlayer` structs plus rosters and recommendations, several times a second during a draft. At this size it is invisible (the soak proves it), but it is the one place the design scales with board size rather than with change.
- **Fix:** Only if it ever matters: memoize the available list keyed on the pick fingerprint that already exists (`view::poll_fingerprint`).
- **Effort:** M
- **Grade lift:** B+ → B+ (headroom, not a fix — do not do this before the draft)

#### G2 — Stream or cap the weekly projections download
- **Where:** `src-tauri/src/sleeper.rs:294-306`; cached file is 18 MB (`weekly_2026.json`)
- **What's wrong:** The weekly file is deserialized whole into `Vec<ProjectionRow>` to compute per-game bonus expectations, and it is the largest thing the app moves. See B1 for the timeout half of this.
- **Fix:** Keep only the stat keys the bonus model reads while deserializing (serde `#[serde(skip)]` on the rest, or a narrower row struct), which cuts both transfer parsing and the cached file.
- **Effort:** M
- **Grade lift:** B+ → A− (the biggest I/O and memory item shrinks)

#### G3 — Virtualize the board if the cap is lifted
- **Where:** `src/components/Board.tsx:107`, `:176-224`
- **What's wrong:** The 200-row cap is what keeps rendering cheap; C2 proposes letting the user lift it, at which point 394 rows × 11 cells render on every poll.
- **Fix:** If C2 lands, add windowing (`@tanstack/react-virtual`) rather than rendering the full list.
- **Effort:** M
- **Grade lift:** B+ → B+ (only relevant once C2 exists)

---

## H — Documentation & Onboarding — B+

The README is genuinely good and unusually honest: what the app does and why each number is computed the way it is, dev/build/test commands, the headless `dump_state` CLI with `--simulate`, the Ask Claude settings table, the replay harness, the browser preview, a 30-row draft-day troubleshooting table keyed by the exact string on screen, data sources, and a file-by-file layout. Today it gained keeper behaviour and the new sync-stale row. Alongside it sit `TRACKER.md` (32 rows of what landed and where), three dogfood reports with screenshots and repro videos, and two archived grade reports. Comments in code explain *why* rather than *what* (`view.rs:130-137` on keepers, `Panels.tsx:96-99` on the live region). What is missing is the layer above the file list: nothing explains the design decisions, and nothing tells a first-time reader which of the four markdown files to read first.

#### ~~H1 — Write the draft-night runbook~~ ✓ done 2026-08-28 (`02321d9`; README "Before the draft")
- **Where:** `README.md` "Draft-day troubleshooting" covers symptoms; nothing covers the sequence
- **What's wrong:** The troubleshooting table answers "this went wrong, what now" but not "it is 16:45, what do I do". Today's audit produced exactly that list — refresh data on good wifi, launch and eyeball the app, check the pill goes green, confirm your slot and keepers — and it lives only in a grade report.
- **Fix:** Add a short "Before the draft" section: the checklist, the two cache TTLs and what expiring means, and the one-line recovery for each of the three most likely failures.
- **Effort:** S
- **Grade lift:** B+ → A− (the operational knowledge stops living in my head)

#### H2 — Add an architecture note
- **Where:** `README.md` "Layout" is a file list; no ADRs
- **What's wrong:** Nothing records *why* the interesting decisions were made: one `DraftView` serving both UI and model, `seq` for ordering live updates, manual picks as a fallback layer that API picks override, the `desktop` cargo feature existing so fuzz targets can link the domain library, the base-URL seam existing for the replay harness. All are load-bearing and all are currently reconstructible only from commit messages.
- **Fix:** One `docs/architecture.md` of ~200 lines: the data flow diagram, those five decisions with their reasons, and the schema-version contract.
- **Effort:** S
- **Grade lift:** B+ → A− (the design becomes transferable)

#### H3 — Point the four markdown files at each other
- **Where:** `README.md`, `TRACKER.md`, `dogfood-output/*/report.md`, `.claude/grade-report.md`
- **What's wrong:** Four overlapping documents with no index. A newcomer cannot tell that TRACKER is the live status, the dogfood reports are evidence, and the grade report is the improvement backlog.
- **Fix:** Three lines at the top of the README saying what each file is for and when it is updated.
- **Effort:** S
- **Grade lift:** B+ → B+ (navigability)

---

## I — Developer Experience & Tooling — B+

`bun run verify` is the single gate and it is comprehensive: LOC cap → `cargo fmt --check` → `tsc` → Vite build → Vitest → `cargo test --all-targets` → Playwright → ESLint `--max-warnings=0` → clippy `-D warnings --all-features`. It ran green a dozen times today and caught real problems each time (the 500-line cap fired twice and forced two genuinely better file splits). Beyond that: three fuzz targets, a replay harness with pause/step/rewind control endpoints, a headless `dump_state` with `--simulate`, and a browser preview that renders a real captured dump. Held at B+ by the same two gaps as this morning: CI has still never run, and there is no pre-commit hook, so `verify` is a discipline rather than a guarantee.

#### ~~I1 — Make CI run (and be the thing that says green)~~ ✓ done 2026-08-28 (PR #1)
- **Where:** `.github/workflows/verify.yml`; 28 unpushed commits
- **What's wrong:** Same as D3 from the other side: the pipeline exists, is well-written, and has never executed. Every "verify is green" claim in three reports today means "green on this laptop".
- **Fix:** Push and open a PR; add `push: branches: ['**']`; add a status badge to the README once it has run.
- **Effort:** S
- **Grade lift:** B+ → A− (green becomes a fact about the code, not the machine)

#### I2 — Add a pre-commit hook running the fast half of verify
- **Where:** no `.husky/`, no `.git/hooks/pre-commit`, no `lefthook.yml`
- **What's wrong:** Nothing prevents committing code that fails `tsc` or clippy; today that was caught only because I ran `verify` by hand between commits.
- **Fix:** `lefthook` (or a plain hook) running `check:loc`, `format:check`, `typecheck` and `lint:frontend` — the sub-10-second subset — leaving the full suite to CI.
- **Effort:** S
- **Grade lift:** B+ → A− (the gate stops depending on memory)

#### I3 — Split `verify` into fast and full
- **Where:** `package.json:14` (`verify` chains nine steps including a Vite build and a full Playwright run)
- **What's wrong:** One target for both the inner loop and the release gate means the inner loop pays for a production build and a browser run every time.
- **Fix:** Add `verify:fast` (LOC, fmt, tsc, unit tests, lint) and keep `verify` as the full gate; point the hook at the former and CI at the latter.
- **Effort:** S
- **Grade lift:** B+ → B+ (loop speed)

#### I4 — Teach the replay harness to inject failures
- **Where:** `draft-assistant/scripts/replay-sleeper.mjs:150-175` (control endpoints: status/step/pause/resume/set)
- **What's wrong:** The harness replays a *happy* draft. Every failure path in the app — 500s, hangs, half-written dumps, a pick vanishing — still has to be produced by killing processes by hand, which is how today's stalled-feed test was run.
- **Fix:** Add `/replay/fail?mode=500|hang|garbage&for=Ns` so the poll-failure, stale-cache and error-boundary paths become one-command reproductions.
- **Effort:** S
- **Grade lift:** B+ → A− (failure testing becomes as cheap as happy-path testing)
