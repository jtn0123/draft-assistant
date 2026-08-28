# Build Prompt: Fantasy Football Draft Assistant

> **Validation status (2026-08-27):** The research tasks below have been executed — repos cloned and assessed, two apps run locally with screenshots, data endpoints tested with live calls. See **"Validation findings"** at the bottom of this file before acting on the original research asks.

## What I want

A local-first draft assistant for a live snake draft. During the draft it tracks every pick as it happens, keeps my roster and everyone else's roster current, and tells me who to take when I'm on the clock. Draft mode only for now — the schema should leave room for in-season features later, but don't build them.

Two hard requirements:

1. **AI-readable back end.** The data model should be trivially queryable by an LLM. I want an endpoint or CLI command that dumps the full current draft state as clean JSON (available players with projections, my roster, opponent rosters, positional scarcity, my next picks) so a model can reason over it without scraping a UI.
2. **Multi-league support.** League config lives in its own table/file. I switch leagues with a dropdown or a flag, no code changes.

Before writing code, research the options below and tell me which starting point you recommend and why. Push back if you think a different architecture is better.

---

## League config (league #1)

**Format:** Sleeper, 14 teams, snake draft, redraft.
**My draft slot:** 2.

**Starting lineup:** 1 QB, 1 RB, 1 WR, 1 TE, 4 FLEX (WR/RB/TE), 1 DEF. **No kicker.**
Bench size: TBD — make it a config value.

That's 7 non-QB/DEF starters per team. Across 14 teams that's 98 RB/WR/TE starting every week. This is an extremely shallow talent pool relative to demand, and it should dominate the valuation model.

**My pick numbers (14-team snake, slot 2):**

| Round | Overall | | Round | Overall |
|---|---|---|---|---|
| 1 | 2 | | 9 | 114 |
| 2 | 27 | | 10 | 139 |
| 3 | 30 | | 11 | 142 |
| 4 | 55 | | 12 | 167 |
| 5 | 58 | | 13 | 170 |
| 6 | 83 | | 14 | 195 |
| 7 | 86 | | 15 | 198 |
| 8 | 111 | | 16 | 223 |

Note the shape: a 25-pick gap after round 1, then picks come in back-to-back pairs three apart (27/30, 55/58, 83/86...). The recommendation engine must account for this — at pick 27 I should be thinking about what survives to 30, not just who's best right now.

### Scoring

**Passing**
- Passing yards: 0.04/yd (1 pt per 25)
- Passing TD: **6** (not the standard 4)
- 2-pt conversion: 2
- Interception thrown: -2

**Rushing**
- Rushing yards: 0.1/yd
- Rushing TD: 6
- 2-pt conversion: 2

**Receiving**
- Reception: **1.0 (full PPR)**
- Receiving yards: 0.1/yd
- Receiving TD: 6
- 2-pt conversion: 2

**Fumbles**
- Fumble lost: -2
- Fumble recovery TD: 6

**Yardage bonuses (per game)**
- 100–199 rushing: +3 | 200+ rushing: +4
- 100–199 receiving: +3 | 200+ receiving: +4
- 300–399 passing: +3 | 400+ passing: +4

**Player special teams**
- ST player TD: 6 | ST forced fumble: 1 | ST fumble recovery: 1

**Team defense**
- Sack: 1 | INT: 2 | Fumble recovery: 2 | Safety: 4 | Blocked kick: 2
- Defense TD: 6 | Special teams TD: 6 | ST fumble recovery: 2 | 2-pt conversion return: 3
- Points allowed: 0 → 12 | 1–6 → 8 | 7–13 → 6 | 14–20 → 4 | 21–27 → 1 | 28–34 → -1 | 35+ → -4

### Scoring implications to bake into the model, not hardcode

Encode scoring as data, then let the valuation fall out of it. But sanity-check your output against these:

- **6-point passing TDs** raise QB value meaningfully over the default rankings you'll find on any site. Most public ADP assumes 4-point passing TDs. Every projection you ingest must be re-scored against this ruleset, not used raw.
- **Only 1 QB starts**, which pulls the other direction. 14 QBs needed from a pool of ~32 starters means replacement level is high. The two effects partly cancel — compute it, don't guess.
- **4 flex spots** make RB/WR/TE replacement level brutally low. Value over replacement should be calculated against the actual 98-player skill pool, not a generic baseline.
- **Yardage bonuses reward volatility.** Two players with identical season projections are not equal if one has a higher game-level ceiling. If you can get game-log distributions, model the bonus expectation rather than applying a flat average.
- **No kicker.** Never surface one.
- **DEF points-allowed goes negative** at 28+. Streaming-viable, low draft priority, but the model should know bad defenses actively cost points.

---

## Data sources — research these

### Sleeper API (primary, confirmed)

Read-only, public, free, no API key or auth. Rate limit is roughly 1000 calls/minute, which is far more than a draft needs. Base URL `https://api.sleeper.app/v1`.

Relevant endpoints:
- `GET /league/{league_id}` — league settings, roster positions, scoring
- `GET /league/{league_id}/drafts` — find the draft, get `draft_id`
- `GET /draft/{draft_id}` — draft metadata, slot-to-roster mapping, status
- `GET /draft/{draft_id}/picks` — **every pick made, in order.** Poll this every 3–5 seconds during the draft and the roster state maintains itself. No manual pick entry needed.
- `GET /players/nfl` — full player dictionary. ~5MB, cache it locally, refresh once a day at most.

Also verify the league's own scoring settings against what I typed above by reading `/league/{league_id}` — if they disagree, trust the API and flag the mismatch.

Confirm the picks endpoint behavior yourself before building on it. Also check whether a websocket exists; polling is fine as a fallback.

### Projections and ADP (needs a decision)

Sleeper gives you players, picks and rosters but **not projections or rankings**. Options to evaluate:

- **`nfl_data_py`** (Python, pip) — historical play-by-play, weekly and seasonal stats, rosters, ID mappings across sites. Sourced from nflfastR/nflverse. Best option for building your own projections and for the ID crosswalk between Sleeper IDs and everything else.
- **`ffanalytics`** (R) — scrapes projections from a dozen public sources and aggregates them, plus ADP/AAV/ECR. Closest thing to turnkey custom-scoring projections. R dependency is the downside; could be run once to produce a CSV rather than wired in live.
- **`ffscrapr` / ffverse** (R) — API wrappers including Sleeper, with caching and rate limiting handled.
- **FantasyPros** — good consensus data, but scraping it has terms-of-service implications. Check before relying on it.

Recommend one. Priority is that projections arrive as raw stat lines (attempts, yards, TDs, receptions) rather than pre-computed fantasy points, because I need to re-score everything under the rules above.

### Open source starting points — evaluate these repos

Look at each, assess code quality and activity, and tell me whether to fork, borrow from, or ignore.

**Closest match — read this one first**

https://github.com/zacharykirby/ai-nfl-fantasy-draft

MIT licensed, Python 3.12, FastAPI. Actively developed for the 2026 season. It already does:
- VORP-based rankings with auditable score breakdowns, tiers, and risk flags
- Crash-safe event-backed draft sessions with snake pick ownership, undo, and atomic autosave
- `safe`, `balanced`, and `upside` recommendation modes — exactly the multi-mode output I described
- League-aware replacement value, tier cliffs, position-run detection, and player/tier survival estimates
- An optional LLM reasoning layer that receives a bounded evidence packet, with deterministic fallback when the model is unavailable
- A versioned REST API (`/api/v1/sessions/{name}/cockpit`, `/board`, `/roster`, `/recommendation`, `/available`) and a mobile web view served over Tailscale
- Fuzzy player name matching and a bulk "catch up" command for missed picks

**Two things it does not do that I need:**

1. **No Sleeper integration.** Picks are entered manually (`live_draft.py draft home-league "Jahmyr Gibbs"`). Adding a Sleeper poller that feeds its existing pick-recording path is the single highest-value change.
2. **Its projections come from ESPN Mike Clay plus FantasyPros DraftWizard ADP**, converted from published PPR totals using projected receptions. That conversion won't survive my ruleset — 6-point passing TDs and per-game yardage bonuses can't be derived from a PPR total. I need raw stat lines.

Also note it treats D/ST as outside skill-position VORP and reserves it for the last two rounds, and it defaults its board to 20 QB / 50 RB / 60 WR / 20 TE. In a 14-team league with 4 flex those depths are too shallow — check `--board-top`.

Caveat: 0 stars, 0 forks, ~50 commits. It's one person's personal project, not a maintained community tool. Judge the code, not the popularity.

**Worth reading, not forking**

https://github.com/gnmerritt/fantasy-bot

Dead — last release 2016, built on bower/brunch/CoffeeScript. But the README documents a flex-aware VORP demand formula (starter slots including flex, plus bench slots, times league size) that is directly relevant to my format, and it honestly lists its own failure modes: it overestimates demand for backup DST/TE, ignores bench composition when drafting depth, and is vulnerable to projection error. Read it as a design lesson, then write your own.

**Others I have not vetted closely — check these yourself**

- https://github.com/joshmgrey/fantasy-draft-consultant — AI draft assistant, risk scoring, draft/pass verdicts
- https://github.com/mattheworres/hootdraft — full web-based draft room; heavier than I need, but look at how it models multi-league state
- https://github.com/jjti/ff — draft assistant pulling projections from ESPN, CBS, and NFL
- https://github.com/NateConroy1/fantasy-draft-helper — simple offline draft manager for compiling your own rankings
- https://github.com/DFrancis84/fantasy_football_draft — Python tool built for running live drafts on paper

Honest assessment please. If the fit is poor across the board, say so and propose building clean.

---

## Functional requirements

**Setup**
- Enter a Sleeper league ID (or draft ID), pull settings, confirm roster and scoring
- Store league configs; switch between them

**During the draft**
- Poll Sleeper for picks, update state automatically
- Manual pick entry as a fallback if the API lags or the draft is offline
- Show board: available players ranked by value under my scoring, filterable by position, searchable
- Show all 14 rosters with positional needs
- Highlight when I'm on the clock and how many picks until my next one

**Recommendations (when I'm up)**
- A **safe** pick and a **balanced** pick, each with a one-line reason
- Value over replacement under my exact scoring rules
- Positional scarcity — how many players at this tier survive to my next pick, given the gap size
- Roster construction fit against my remaining starting slots
- Flag reaches and flag value falling past ADP

**AI interface**
- One call returns the entire draft state as structured JSON
- Optional: an LLM call for a natural-language read on the recommendation, with the API key in an env var, and the app fully functional with it disabled

**Non-functional**
- Runs locally, no account required
- Fast — I have limited time on the clock
- Handle API failure gracefully; a dead network shouldn't kill the draft
- Cache the player dictionary

---

## Deliverables from you, before code

1. Which repo (if any) to start from, and why
2. Which projection source, and why
3. Proposed stack and schema, with the multi-league and AI-readability requirements addressed
4. The valuation approach, in enough detail that I can argue with it
5. Anything I've specified that you think is wrong

---

# Validation findings (2026-08-27)

Everything below was verified empirically on this machine — live API calls, local clones, running apps.

## Data sources — RESOLVED

**Sleeper has its own undocumented projections API, and it changes everything.** One unauthenticated call returns raw stat lines (pass_att/yd/td/int, rush_att/yd/td, rec/rec_yd/rec_td, fum_lost, 2pt, games played) for ~3,100 players, already keyed by Sleeper player IDs, plus ADP in every scoring format:

```
GET https://api.sleeper.app/projections/nfl/2026?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&order_by=adp_ppr
```

Weekly per-game projections also exist (`/projections/nfl/2026/{week}` and `/v1/projections/nfl/regular/2026/{week}`) — usable for modeling the per-game yardage bonuses instead of a flat average. Verified live 2026-08-27; data is Rotowire-sourced, updated 2026-08-23. Caveat: undocumented, so snapshot every fetch and validate the schema on read.

This satisfies the "raw stat lines, not pre-computed points" requirement with zero scraping, zero R, and zero ID-crosswalk work.

Other sources, verified:
- **ESPN `kona_player_info` API** — works, raw stats, independent projection set; good as a second opinion. Stats keyed by numeric IDs needing a mapping table.
- **nfl_data_py is deprecated** (archived Sept 2025) → use **`nflreadpy`**. Historical stats only, NO forward projections — its real value is `load_ff_playerids()`, a 12k-player crosswalk (sleeper_id ↔ espn_id ↔ fantasypros_id ↔ …).
- **ffanalytics (R)** — decaying; its FantasyPros scraper silently truncates to 10 players/position (open issue, independently reproduced). Skip.
- **FantasyPros** — v2 API is key-gated (403 without key); anonymous scraping now returns only 10 server-rendered players per position. Skip unless a free personal API key is requested later.
- **Sleeper core API** — `/state/nfl`, `/players/nfl`, and the projections endpoints verified live. Note the player dictionary is ~14.6MB now, not 5MB. The `/draft/{draft_id}/picks` polling behavior still needs a live test against the actual league's draft ID (or a mock draft) — supply the league ID.

## Repo verdicts

**zacharykirby/ai-nfl-fantasy-draft — RUN LOCALLY, VERIFIED WORKING. Borrow heavily; probably don't fork.**
Set up in `research/ai-nfl-fantasy-draft` (Python 3.12 venv). The checked-in 2026 board (Aug 5 data, ~340 skill players) validates and serves. Created a 14-team/slot-2 session, recorded picks via API, screenshotted the web cockpit — see `research/screenshots/`. What the README promises is real: tier-aware board, survival-to-next-pick estimates, auditable score components, undo, tier/run alerts, and `GET /api/v1/sessions/{name}/cockpit` returns the full draft state as one ~22KB JSON — exactly the AI-readability requirement.
Confirmed gaps, all as the spec suspected:
1. No Sleeper integration (manual pick entry only).
2. Scoring hardcoded to standard/half_ppr/ppr — no custom rules engine, no 6-pt pass TD, no yardage bonuses. League size/starters/flex ARE configurable, and the flex-aware VORP allocation is sound.
3. Kickers are baked into board roles (spec says no K).
4. Board health gate blocks sessions when projections are >14 days stale, and the refresh path scrapes FantasyPros/ESPN with Selenium (FantasyPros scraping is now broken — see above). For the demo the metadata timestamp was bumped manually (demo shim, flagged).
Also: ~12K LOC of one person's Python. Solid domain logic, but the projection pipeline is its weakest layer — and that's exactly the layer the Sleeper projections API replaces.

**jjti/ff (ffdraft.app) — RUNNABLE, TypeScript, actively maintained.** Next.js 14 + React 18 + Redux, TS-strict-ish, commits through July 2026 with working 2026 scrapers and a checked-in `Projections-2026.json` (503 players, raw stat lines averaged from ESPN/CBS/NFL). Per-stat scoring weights are editable per league — including 6-pt passing TDs and full PPR — but scoring is linear on season totals: no per-game yardage bonus modeling. Snake with configurable team count and roster slots. No Sleeper sync, no VORP audit trail as rich as the Python app. Run locally in `research/ff/app` (2026 data swapped into `public/projections.json`).

**gnmerritt/fantasy-bot — dead as expected; formula extracted.** Per-team positional demand = Σ over roster slots of 1/(eligible positions in slot) — i.e. "RB/WR" adds 0.5 to each, a 4-way flex adds fractional demand to RB/WR/TE. Replacement rank = ceil(demand × teams), smoothed by averaging the next 3 players. Its own documented failure modes to design against: bench demand ≠ starter demand by position (backup TE/DST ≈ 0), bench composition ignored when drafting depth, raw-VORP fragility to projection error (→ use tiers/uncertainty bands).

**mattheworres/hootdraft — don't run it.** PHP 7/Silex (EOL), MySQL, Vagrant, SMTP + reCAPTCHA for accounts; multi-tenant hosting plumbing irrelevant to a single-user tool. One schema idea worth stealing: everything hangs off a `draft` row that owns the pick cursor plus a monotonically incremented `draft_counter` that clients poll for cheap change detection; picks are first-class rows keyed (draft_id, round, pick, overall).

**Others:** joshmgrey/fantasy-draft-consultant = a Stripe-paywalled "ask Claude about one player" widget, not a draft tool — ignore. NateConroy1/fantasy-draft-helper = CSV-list tracker, Gatsby 2 won't build on modern Node — ignore. DFrancis84/fantasy_football_draft = a pick logger typing into SQLite — ignore.

## Where this leaves the build decision

No repo satisfies the two hard requirements together (Sleeper-live + custom scoring engine). The two best assets are:
- the **valuation/draft-session domain logic** in ai-nfl-fantasy-draft (Python), and
- the **Sleeper projections + picks APIs**, which eliminate the scraping layer every one of these repos struggles with.

Open decision — owner's stack preference is a fully type-safe app (TypeScript and/or Rust; desktop now, possibly Android tablet later), which argues for building clean (e.g. Tauri + React/TypeScript strict, or TS web app) with the Python repo used as a reference implementation rather than a fork. To be discussed before any code.

---

# Decisions (2026-08-28, after validation review)

Owner reviewed both running demos and decided:

1. **Build clean.** No fork. The Python repo (ai-nfl-fantasy-draft) is a reference implementation for cockpit features; jjti/ff is the UI/UX reference.
2. **Stack: Rust core + fully type-safe frontend, packaged with Tauri 2.** Rationale: owner wants a Rust backend and full type safety end to end; Tauri 2 ships the same Rust core + web frontend as a Mac desktop app now and an Android app later **with no server on the tablet** — the Rust engine compiles into the app on both platforms. Frontend: React + TypeScript in strict mode (React with TS strict is fully type safe; plain-JS React is not).
3. **UI/UX modeled on ffdraft.app** — the simple ranked board with live re-ranking — plus the cockpit features worth porting from the Python app: tier/run alerts, survival-to-next-pick estimates, auditable recommendation reasons, undo, on-the-clock banner. **Dark mode required** (the Python cockpit's dark theme is a good visual reference).
4. **API layer is baked in, not optional:** the one-call full-draft-state JSON dump ships from day one (local endpoint and/or CLI/file export on device).
5. **LLM layer baked in but provider-optional:** deterministic engine always works; an LLM "explain/advise" layer activates when a key is present (owner will supply an OpenAI token later; until then Claude-in-session reads the state JSON directly).
6. **Sleeper usage model confirmed:** owner drafts in Sleeper as normal; this app is a read-only second screen polling the public API. No login, no account credentials, no writes to Sleeper.

---

# League verified against the live API (2026-08-27)

League ID **1389710366300200960** ("UMass Wrestling Fantasy Football League", 2026, 14 teams) and draft ID **1389710366300200961** confirmed. Raw JSON cached in `research/league.json` and `research/draft.json`.

- **Scoring: every value in this doc matches the league exactly** (diffed programmatically; only float32 noise on the 0.1/yd values). Kicker scoring keys exist in the config but there is no K roster slot, so they're inert.
- **Roster confirmed:** QB, RB, WR, TE, FLEX×4, DEF + **6 bench** (bench size was TBD in this doc — it's 6).
- **Corrections to this doc:**
  - The draft is **15 rounds, not 16** — the pick table's round-16 row (pick 223) doesn't exist. Slot 2's last pick is 198.
  - Player dictionary is ~14.6MB, not ~5MB (noted earlier).
- **Draft order is already set.** Slot 2 = Sleeper user `mcsleeper26`. Snake, 90-second pick clock.
- **Picks endpoint verified live** — returns `[]` pre-draft, as expected.
- **⚠️ The draft starts 2026-08-28 at 5:00 PM PDT — about 23 hours after this verification.** The full Tauri build cannot land by then; a draft-night rig using the validated pieces (running cockpit/board + Sleeper polling + Claude reading the state JSON) is the realistic path for this draft, with the Tauri app built properly afterward.

---

# Build status (2026-08-27 evening — draft is tomorrow 5 PM PDT)

**v0.1 is built, tested, and running.** `draft-assistant/` is a Tauri 2 app: Rust core (9 unit tests passing, zero warnings) + React/TypeScript-strict frontend, all files ≤500 LOC. Release bundle at `draft-assistant/src-tauri/target/release/bundle/macos/Draft Assistant.app` (plus a .dmg). See `draft-assistant/README.md` for architecture and commands.

Working, verified against the real league:
- Board of 419 players scored under the exact ruleset (dot product with league scoring_settings + per-game bonus expectation from weekly projections); Sleeper pts_ppr kept per-player as an auditable cross-check
- VORP with ADP-allocated flex demand — lands on exactly the 98 RB/WR/TE startable pool; QBs gain ~50–70 pts from 6-pt passing TDs but rank ~35+ overall because 14-team QB replacement is 341.8 (the "two effects partly cancel" prediction, computed)
- Live pick polling (3s, auto-starts), on-the-clock banner, tier alerts, position-run detection, survival-to-next-pick odds tuned for the 27/30-style double picks, balanced+safe recommendations with reasons, manual pick fallback + undo, bye weeks inferred from weekly opponent data, real team names from /league/users
- AI dump: "Export state" button, `get_state` command, and `cargo run --bin dump_state -- <league> [user] [out] [--simulate N]` (the simulator fakes N picks to preview mid-draft states)

Deferred (post-draft): LLM provider layer (OpenAI token pending — Claude-in-session reads the state JSON meanwhile), Android/Tauri-mobile packaging, richer opponent-roster UI, upside recommendation mode, tier model refinement past the top ~50.

## Hardening pass (2026-08-27 late evening)

Full-draft simulations (210 picks) + invariant validation at 5 draft states (no duplicate picks, snake ownership correct, drafted∉available, survival∈[0,1], no kickers, roster counts exact). Bugs found by simulation and fixed, with unit tests (13 passing, clippy clean):
- Recommendation engine drafted 4 DEFs / 1 RB over a full autopilot draft — the exact bench-composition failure fantasy-bot documented. Added positional discipline (never a 2nd DEF or 3rd TE/QB, backup penalties, thin-position fragility boost).
- VORP score normalization exploded late-draft when board-best VORP was small (negative-VORP players scored -300); replaced with a fixed scale.
- Candidate pool was top-60 overall, which buried late-round RBs under WRs; now also includes top 10 per position.
Autopilot full-draft roster now: 2 RB / 9 WR / 2 TE / 1 QB / 1 DEF, all starters filled (WR-heavy is rational under 4-flex full PPR).

Note for the LLM layer: an OpenAI ChatGPT subscription does NOT include API access — an API key is a separate pay-as-you-go account at platform.openai.com.
