# Draft Assistant — grade & tracker (2026-08-30)

## Current focus: stability · performance · lazy load · week-one assistant · status

| # | Item | Status | Notes |
|---|---|---|---|
| 1 | Lazy-load the big UI chunks (Chat, SeasonScreen, DraftScreen) | done `b4f2c39` | Entry bundle 273 → 228 KB; each screen is its own chunk, Chat only fetched when opened. |
| 2 | Board sort caching without staleness (audit G7) | done `c8ca01e` | New `boardIdentity.ts` deep-compares players and reuses the array only when observationally identical — re-sort and ~400 row re-renders skipped on no-op ticks, staleness impossible by construction. |
| 3 | Per-source status + analysis age | done `4cecd63` | Live badge tracks matchups/scores/rosters separately with a plain-language tooltip; trade/waiver ideas show "from N min ago" when the cached analysis is older than 2 min. |
| 4 | Week-one assistant upgrades (reasons, injury flags, decision deadlines) | done `bb61687` | New `season_calls.rs` (advice) + `season_injury.rs` (Sleeper's dozen injury spellings → Q/D/O). Every call carries a plain-language `reason` and a `locks_at_ms` deadline from the scoreboard's `start_time` ("decide by Sun 1:00 ET"); the head-to-head table tags injured starters on both sides with the word on hover; a starter listed Out/Doubtful raises a call the projections alone would miss, sorted to the top and kept out of the "points on the table" total. All fields additive/optional, so the checked-in fixture still loads. `matchup_rows` moved to `season_view_parts.rs` to keep `season.rs` under the cap. |
| 5 | Split `build_season_view` by section (audit A5) | done `c144643` | `season_view_parts.rs` dissolved into six section modules — `season_lookup.rs` (the Lookup primitive), `season_view_matchup.rs` (head-to-head + start/sit calls), `season_view_live.rs`, `season_view_standings.rs`, `season_view_market.rs` (waivers + trades) and `season_view_feeds.rs`. `season.rs` drops 480 → 340 lines and keeps only `SeasonView`, `SeasonAnalysis` and the orchestration; no section file exceeds 250 lines or 10 imports. `MatchupSection` hands its working values forward so nothing is recomputed. Audit A2's `matchup_for`/`opponent_of` moved to `season_api.rs` on the way; `why_start` stayed with the calls plumbing rather than going into the pure lineup solver. Golden tests passed unmodified — zero behaviour change. |

Follow-ups found during this work:
- `DraftPollMemory::picks_changed` keys on pick *count* only — a commissioner replacing a pick without changing the count never reaches the UI (`poll.rs`). done `a1af9e0` — now compares count plus a hash of every pick number and player id.
- Pre-commit hook was broken for git worktrees (validated the main checkout). done `b4f2c39`

---
## Earlier tracker (Claude Design import)

Grades are for the working tree after the Claude Design import (nothing is committed yet).
Evidence: section-by-section audit of `Draft Assistant.dc.html` vs `src/` + `src-tauri/src/`,
screenshots of every screen/tab in both themes, and live Sleeper data for the lineup bug.

| Area | Grade | One line |
|---|---|---|
| Design fidelity | B+ | 78 elements match, 24 partial, 6 missing, 8 deliberately changed. All 38 colour tokens hex-identical. |
| Draft screen UX | A− | Board, rec cards, snake strip, at-risk, tier alerts all real and wired. Missing the pick clock. |
| Season screen UX | B | Looks finished, but the headline "calls to make" panel is wrong in a common case (A1). |
| Backend | B+ | 72 Rust tests, deterministic odds, graceful degradation. Two logic bugs + one tie-break gap. |
| Ask Claude | B− | Real Messages API, key handling, effort/model — but copy is placeholder-ish and it's Anthropic-only. |
| Code quality | A− | `npm run verify` green (33 FE + 72 Rust tests, eslint, clippy -D warnings, 500-LOC cap). |
| **Overall** | **B+** | Everything visual landed; three correctness bugs and one missing design feature keep it off A. |

Status legend: `todo` · `in progress` · `done <commit>` · `deferred (why)`

## A. Correctness bugs (found in screenshots, confirmed in code / live data)

| # | Item | Status | Notes |
|---|---|---|---|
| A1 | "Your lineup is already optimal" is wrong when the set lineup is a permutation of the optimal one. `calls_from_diff` pairs by slot index; when Garrett Wilson is set at WR and Christian Watson at FLEX (optimal has them swapped), the real call — **Josh Downs over Tony Pollard, +points** — is skipped. Confirmed against live Sleeper starters for roster 2, week 1. | done | `calls_from_diff` now pairs incoming players with an outgoing starter in a slot they can fill; regression test from the live week-1 roster. Fixture regenerated: "Josh Downs over Tony Pollard, +2.8". |
| A2 | League activity shows every transaction twice in week 1. The fetch loop is `[week-1 max 1, week]` → `[1, 1]`. | done | Week list deduped and transactions deduped by id in `season_engine.rs`. 12 unique activity rows on the live league. |
| A3 | Standings at 0–0 are ordered by roster id (AllDay21 1487 proj at #1, Northern CT 1915 at #3). | done | Standings tie-break on projected total; test `level_teams_are_ordered_by_projection_not_roster_id`. |
| A4 | Last season lists two teams at place 1 (regular-season #1 and the champ) then jumps to 3. | done | Champion sorts first; places are 1..n with no repeats. |
| A5 | My team tab shows `0.0` for every player in week 1 (season-to-date only). Reads as broken pre-kickoff. | done | `RosterRow.projected` (this week) + "Wk"/"Season" columns in My team; test in `SeasonTabs.test.tsx`. |

## B. Design gaps (implementation vs prototype), by user impact

| # | Item | Status | Notes |
|---|---|---|---|
| B1 | **No pick clock** — banner "0:41" cell and on-clock chip timer absent; `pick_timer` unused. | done | `Draft.last_picked` + `pick_timer` → `DraftStatus.clock_deadline_ms` (drafting only); `useClock` ticks 1s in `ClockBanner.tsx`, banner cell + on-clock chip, clamps 0:00. |
| B2 | Recent picks show raw overall pick (`27`) and no team logo; design: `3.03 [logo] Chase Brown`. | done | `RecentPick.team` added; `Panels.tsx` shows `pickLabel` + team logo. Fixture updated. |
| B3 | Lineup margin lacks "· 62% to win" suffix. | done | `LineupCompare` takes `winOdds`; margin reads "+13.5 · 62% to win". |
| B4 | Games tab: footer "Byes this week: DEN, LAC · updates every 30s" missing; roster line says "Them:" not the opponent's name. | done | `season_live::bye_teams` (32 codes minus the slate) → `LiveSection.bye_teams`; footer + opponent name on roster line in `GamesTab.tsx`. |
| B5 | Chat context line drops model + effort ("Sees this draft · pick 3.04 · Opus 5 · high effort"); empty-chat copy says "who to take" on the Season screen. | done | Chat appends "· Opus 5 · high effort" to the context line; empty-thread copy is per screen. |
| B6 | Header meta lacks scoring format ("14-team · 15 rounds" vs "12-team half-PPR · 14 rounds"). | done | `scoringFormat(rec)` in `format.ts` → "14-team full-PPR · 15 rounds". |
| B7 | Position-run note loses its count ("RB run in progress — 4 of the last 6"). | done | `position_run` is now `PositionRun { position, count, window }`; rendered "RB run in progress — 4 of the last 6". |
| B8 | Launch screen never shows the league name/id being restored. | done | App keeps the `StoredLeague` being restored; launch card renders "Restoring **Name** (id)". |
| B9 | Waivers head shows "$1000 left" not "$38 of $100 left". | done | `waiver_budget_total` exported from `season.rs`; head reads "$38 of $100 left". |
| B10 | Search placeholder "press /" + shortcut missing; loading/toast copy drop player count. | done | Placeholder "Search players — press /", global `/` focuses search (ignored while typing); loading note + refresh toast name `board_size`. |
| B11 | Last-season narrative foot and "Lineup" activity kind (empty starter slot) don't exist. | partial | "Lineup" activity kind lands via `season_moves::lineup_gaps` (empty starter slot per roster). Last-season narrative foot deferred: needs bracket data the backend does not fetch yet. |
| B12 | Minor: model-button tooltips, `a:hover` colour, at-risk survival always orange (design: only ≤25%), whole bench row dimmed, column widths, Fable "Ultra" level omitted. | done | Model tooltips, `a:hover` #1e4f36 (dark keeps --pos), at-risk survival act only ≤25%, only bench points cell dimmed, settings menu 274px. Fable "Ultra" level not added — `chat.rs` is owned by the other engineer. |

## C. Deliberate deviations (already made — flag if you disagree)

| # | Item |
|---|---|
| C1 | Appearance follows the OS by default with a System→Light→Dark override (design: toggle only). Your call from the scope question. |
| C2 | "Export board · CSV" became "Export state · JSON" (the existing `export_state` command). |
| C3 | Waiver foot "Claims process Wednesday 3am ET" replaced — waiver day isn't fetched from league settings. |
| C4 | Ask Claude reaches Claude either through the Claude Code CLI (your subscription — now the default when installed) or an API key. No OpenAI route; say so if you still want one. |

## D. Projections script (`research/ai-nfl-fantasy-draft/scripts/fetch_2026_projections.py`)

What it produces: 482 rows — ESPN Mike Clay season projections (PDF-parsed, converted to **half-PPR** by receptions), FantasyPros mock-draft ADP, K/DST from FantasyPros, bye weeks, ADP-bin tiers. 57 rows are ADP-curve estimates, 156 have no ADP. Run 2026-08-28.

What the app already has: Sleeper stat-line projections × **your exact league scoring** (full PPR, 6-pt pass TD, yardage bonuses) and Sleeper ADP (the ADP that actually predicts when *your* league-mates pick).

| # | Option | Verdict |
|---|---|---|
| D1 | **Second-opinion column on the draft board** — import the CSV, match by normalised name+position, show Clay's positional rank next to the app's, highlight disagreements ≥ 8 spots, and add a rec-card reason ("Clay has him WR9 — market is 3 rounds late"). Ranks, not points, so the half-PPR vs full-PPR mismatch doesn't matter. | **Recommended.** Highest value per line of code; no new tab. |
| D2 | Dedicated "Projections" tab: per-player source comparison + the metadata manifest (published vs estimated, missing ADP/team). | Nice for auditing, low draft-night value. Do after D1 if wanted. |
| D3 | Use FantasyPros ADP as a second ADP source for survival. | Skip — Sleeper ADP is the right signal for a Sleeper draft. |
| D4 | Season use: "vs preseason expectation" pace on My team / trade ideas (buy-low, sell-high). | Good week-4+ feature; rank-based to dodge scoring mismatch. |
| D5 | Port the script to Rust. | No — PDF + pandas scraping stays Python. Import the CSV via Settings ("Import projections CSV") into the app data dir. |

Note: rerun the script with `--scoring ppr` before importing; it still won't reflect 6-pt TDs or bonuses, which is why D1 uses ranks.

## E. Ask Claude via Claude subscription (requested 2026-08-30)

| # | Item | Status | Notes |
|---|---|---|---|
| E1 | New chat provider "Claude Code (subscription)": app spawns `claude -p --output-format json --model … --effort … --system-prompt … --tools ""` on this Mac; no API key needed. Provider picker in the chat panel; API key stays as the alternative. | done | `chat_cli.rs` spawns `claude -p --output-format json --model … --effort … --system-prompt … --tools "" --no-session-persistence`; prompt over stdin; 240s timeout; login failures explained. `set_chat_provider` command + "Via: Claude Code / API key" picker in the panel; CLI is the default whenever it is installed and no key is stored. Tests in `chat_cli.rs`, `commands_chat.rs`, `Chat.test.tsx`. |

## F. Trends — week-by-week team strength + why it moved (requested 2026-08-30)

| # | Item | Status | Notes |
|---|---|---|---|
| F1 | Snapshot every team's projected strength (best-lineup points per week, rest of season) on each Season load, stored in the app data dir; at most one snapshot per 6h unless the league changed (new transaction / injury). | done | `season_history.rs`: snapshot per Season load, stored as `history_<league>.json`; min 6h gap unless any roster changed; capped at 400. |
| F2 | Attribute each change between snapshots: trades, adds/drops, injuries, projection moves — per team, with the point swing. | done | `season_trends_view.rs`: per-team delta between snapshots with up to three reasons — traded X for Y, claimed X for $n, added/dropped, "X now Out (−8.0/wk)", "X projection +1.2/wk". |

## G. Second round of asks (2026-08-30)

| # | Item | Status | Notes |
|---|---|---|---|
| G1 | Player headshots from Sleeper on every player name (board, rec cards, roster, lineup, calls, waivers). | done | `Headshot` in `bits.tsx` → `sleepercdn.com/content/nfl/players/thumb/<id>.jpg`, team logo fallback for DEF / missing images. |
| G2 | Season is the default screen and sits left of Draft; last choice remembered. | done | `da.screen` in localStorage; storage-safe. |
| G3 | League activity rows show when each move happened. | done | Eastern date + time on every row. |
| G4 | Settings saved safely. | done | Config written atomically (temp → rename), `config.json.bak` kept and used if the live file is corrupt, file mode 0600. API key moved into the **macOS Keychain** (`security` tool) — existing plaintext key migrates on next launch; file fallback elsewhere. |
| G5 | Right rail scrolls independently of the main column. | done | `season.css`: fixed-height body, each column `overflow-y: auto`; single scroll under 900px. |
| G6 | Show current trades and the lock time. | done | League tab now opens with **"Trades in the league"** — every completed trade this week and last, both sides named, timestamped, yours highlighted. Sleeper does not expose pending offers. Lock time was already the "Locks in" header stat; it now also shows the absolute kickoff ("Thu Sep 10 · 8:15 PM ET"). |

| G7 | Settings toggle Headshots / Team logos; photos fetched once and saved. | done | `headshots.rs`: each photo downloaded from Sleeper once into `<app data>/headshots/`, served as a data URL, misses remembered for 3 days, refreshed monthly; frontend session cache asks the backend once per player. Settings → "Player pictures". |
| G8 | Rows keep the same spacing in either picture mode. | done | The avatar is one 22px round slot (20px on board rows) filled by a photo, a team mark, or a blank for teamless players — `.avatar` in `components.css`. Toggling Settings no longer reflows a row. |
| G9 | Trends said "graph" but showed what looked like ruled lines. | done | With only two readings, fourteen near-flat lines were indistinguishable from the gridlines. Chart mode now shows a ranked dot plot — every team on one 100–150 pts/wk scale, mine in green, a tail back to where it started, the change at the right — and switches to the line chart from the third reading on, once a line has something to say. |
| G10 | Trades accepted but not yet processed. | done | This league has `trade_review_days: 1`, so an accepted trade sits for ~a day before it counts. The app used to keep only `status == "complete"` and silently drop the rest; it now lists anything not voted down, tagging an unprocessed one **In review** ("1 in review · 3 completed"). A trade that is only *proposed* is private to the two managers and never reaches the public API. |
| G11 | Show which network each game is on. | done | Sleeper's scores feed carries `metadata.channel` ("CBS", "NBC/Peacock", "Netflix"); plumbed through `LiveGame.channel` and shown beside the kickoff time in the Games tab. Dropped once a game is final — by then it is a where, not a what. |
| G12 | Slot margin moved between the two teams. | done | Both views. Scoreboard: the far-right column became the middle one. Table: the "Margin" column and its centred bar were replaced by a **Gap** column between the two Proj columns. Centred in the column and signed from my side — `+14.1` green for a slot I win, `−0.8` orange for one I lose, `—` when level. (Arrows were tried and dropped: the sign reads faster.) The column is also the divider between the sides: a filled grey band runs unbroken from the **Gap** header down through the last slot in both views (negative margins swallow the row padding and row gap), and the column is a fixed 58px so the header band lines up with the body. |
| G13 | See the lineup you have set, not just the best one. | done | `MatchupView` now carries `set_rows` / `set_projected` alongside the best lineup. A **Best / Set** toggle sits beside Table/Scoreboard; the flag next to the title reads "2.8 sitting on your bench" (or "your lineup is already your best"), and the header score follows the toggle. |
| G14 | Manager pictures across the league views. | done | `LeagueUser.avatar` and the custom `metadata.avatar` are now read; `SeasonView.team_avatars` maps roster_id → reference. `headshots.rs` generalised into a `cached_image` used by both photos and avatars, with `avatar_target` refusing anything but a Sleeper hash or a sleepercdn `/uploads/` URL. Pictures now appear in Standings, the matchup header, Trends (chart legend and dot plot), completed trades (both sides) and trade ideas (both players plus the partner). A manager with no picture gets their initial, so rows never reflow. |
| G15 | Every picture opens larger on click. | done | `zoom.ts` holds the one picture being shown; `Zoomable` wraps each face in a button and `ZoomLayer` (mounted once in `main.tsx`) draws it at 240px with a caption, closing on Escape, the backdrop, or Close. Manager pictures fetch Sleeper's 280px copy for the zoom — the 80px thumb would not stand up — while player photos are already 350px, so the cached one is reused. |
| G16 | League activity was a wall of text. | done | `ActivityItem` now carries `roster_id` and the `player_ids` in the move. Each row shows a tinted kind chip (trade green, waiver/lineup orange, adds grey), the manager's picture beside the sentence, and the faces of the players involved underneath. Split `season_moves.rs` into `season_activity.rs` to stay under the LOC cap. |
| G17 | Everything committed. | done | All season-mode work committed as `25b1c38` (83 files); tests/polish committed separately after. |
| G18 | Fresh codebase grade. | done | Full 9-category audit written to `.claude/grade-report.md` (2026-08-30): overall **B−**, up from C+ on Aug 27. Top leverage: B1 div-by-zero on remote data, D1 sleeper.rs wire tests, B2 refresh_live truthfulness, G5 cached analysis on the 30s poll, E1 CSP. |
| G19 | Coverage to 80%+. | in progress | Frontend: 71.8% → **87.7% lines** (112 tests; new api.ts/SeasonTabs/Chat/App suites) with an 80% threshold now enforced in `vitest.config.ts`. Rust: 57.3% at audit; new `tests/` suites for season view assembly, wire parsing, engine caching landing now. |
| G20 | Polish from the audit. | done | Shared `ordinal()` in `format.ts` (the two diverged copies deleted); season load failure now renders as an error with a **Try again** button instead of a spinner caption; `coverage/` gitignored; CSP set in `tauri.conf.json` (was `null`). |

## Verification
- `npm run verify` — exit 0 on 2026-08-30 after A/B/E/F landed: check:loc, rustfmt, tsc, vite build, 90 Rust unit + 2 integration tests, 59 frontend tests, eslint --max-warnings=0, clippy -D warnings.
- Claude Code route exercised from the shell with the app's exact flags (`-p --output-format json --model claude-opus-5 --effort low --system-prompt … --tools "" --no-session-persistence`).
- Screenshots: `/tmp/ux-shots/01–11` (draft, season × 5 tabs, scoreboard, chat, settings, light + dark).

## Deferred
- B11 last-season narrative foot (needs bracket data the backend does not fetch).
- Fable 5 "Ultra" effort level (a Claude Code feature, not an API effort).
- Nothing is committed — all work is in the working tree.
