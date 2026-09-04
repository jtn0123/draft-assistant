# Yahoo Fantasy as a second platform — findings (2026-09-03)

Question: could this app drive a Yahoo league the way it drives a Sleeper one?
Short answer: yes for the draft board, with a login step Sleeper never needed,
and with projections/ADP coming from our own sources rather than Yahoo.

## What Yahoo offers

- **Official REST API** (`https://fantasysports.yahooapis.com/fantasy/v2/...`),
  XML by default, `?format=json` for JSON. Resources: game, league, team,
  player, roster, transactions, standings, scoreboard, settings, **draftresults**.
- **Draft in progress is readable.** `league/<key>/draftresults` returns every
  pick so far — "if this is called during the draft this includes the players
  that have been drafted thus far" — with `pick`, `round`, `team_key`,
  `player_key` (and `cost` for auctions). League `settings` carries
  `draft_status` (`predraft` / `draft` / `postdraft`), `draft_time`, `draft_type`.
  So the Sleeper poller's loop (poll picks, diff, rebuild board) maps directly.
  No push/websocket API; polling is the only option, same as today.
- **Players**: `players;position=..;status=A` for free agents, stats, percent
  owned. **No projections and no ADP** are exposed. Our board already builds
  from Sleeper projections + the second-opinion CSV, so Yahoo would supply
  the league, the rosters and the picks; the values stay ours. Player identity
  needs a Yahoo→Sleeper id crosswalk (name + team + position match, the same
  normaliser the second-opinion import uses).

## The cost: OAuth 2.0, always

- Every fantasy call needs a user token, even for a public league. Sleeper's
  "paste an ID, no account" model does not exist on Yahoo.
- Register an app at `developer.yahoo.com/apps/create` ("Installed
  Application", Fantasy Sports → Read). Yahoo issues a client id **and a
  client secret**; the token endpoint requires the secret in a Basic header
  (no PKCE-only public client). A desktop app therefore ships the secret —
  every open-source Yahoo tool does this and Yahoo tolerates it for read scope.
- Flow: open `https://api.login.yahoo.com/oauth2/request_auth` in the browser
  with `scope=fspt-r` (Fantasy Sports, read only), redirect to
  `http://localhost:<port>` — plain HTTP, which is what a loopback listener can
  actually serve and what Yahoo accepts for one; `oob` also works and shows a
  code to paste, and is what this app registers by default. Swap the code at
  `/oauth2/get_token`. Access tokens last 1 hour; refresh tokens do not expire
  (they survive password changes) — store the refresh token in the Keychain
  next to the Anthropic key via `secrets.rs`.
- Rate limits are undocumented; Yahoo "may throttle excessive use". A throttled
  caller gets **HTTP 999**, Yahoo's own status, rather than the documented 429 —
  `yahoo.rs` treats both as retryable and tells the user to wait rather than
  showing them the number. Wrappers report failures only on
  many-thousands-of-calls backfills, so a 3-second draft poll is fine.
- Terms: one developer account, no reverse engineering, attribution line
  "Fantasy data provided by Yahoo Fantasy" somewhere in the UI.

## What it would take here

1. `yahoo.rs` client: token store + refresh, `get_json` with the Bearer header,
   the three loaders (league+settings, teams/rosters, draftresults).
2. A `Platform` enum on `StoredLeague` (`sleeper` | `yahoo`) so the picker and
   config carry both; `add_league` dispatches on it. Yahoo keys look like
   `449.l.12345`, so the paste box can tell them apart from Sleeper's digits.
3. Settings → "Connect Yahoo" (opens the browser, catches the redirect on a
   localhost listener — bounded at five minutes, so an abandoned sign-in does
   not hold the port — saves the refresh token).
4. Id crosswalk Yahoo player → Sleeper player for projections and headshots.
5. Season screen: matchups/scoreboard/transactions exist, so Season could
   follow later; the draft screen is the first deliverable.

Estimate: 2–3 days for the draft screen, mostly the OAuth plumbing and the
crosswalk. Nothing in the board, recommender or chat needs to change.

## Scoring stat ids

Yahoo names a scoring rule by a numeric `stat_id`; `yahoo_map::YAHOO_STAT_IDS`
is the crosswalk to the Sleeper key space `crate::scoring` works in. The ids a
football league actually uses:

| id | Yahoo category | app key |
| --- | --- | --- |
| 4 / 5 / 6 | Pass Yds / Pass TD / Int | `pass_yd` `pass_td` `pass_int` |
| 9 / 10 | Rush Yds / Rush TD | `rush_yd` `rush_td` |
| 11 / 12 / 13 | Rec / Rec Yds / Rec TD | `rec` `rec_yd` `rec_td` |
| 14 | Return Yds | *(none — Sleeper splits kick and punt returns)* |
| 15 | Return TD | `st_td` |
| 16 | 2-Point Conversions | `pass_2pt` `rush_2pt` `rec_2pt` |
| 17 / 18 | Fumbles / Fumbles Lost | `fum` `fum_lost` |
| 19–23 | FG made, by range | `fgm_0_19` … `fgm_50p` |
| 25–28 | FG missed, by range | `fgmiss_0_19` … `fgmiss_40_49` |
| 29 / 30 | PAT Made / PAT Missed | `xpm` `xpmiss` |
| 32–37 | Sack, Int, Fum Rec, TD, Safety, Blk Kick | `sack` `int` `fum_rec` `def_td` `safe` `blk_kick` |
| 50–56 | Points Allowed, by band | `pts_allow_0` … `pts_allow_35p` |

An id with no row here is not scored, because guessing a key would add a wrong
number to every player. A league that pays for one gets a warning on the health
strip naming the category, rather than a board that is quietly wrong
(`yahoo_map::unscored_stats_warning`).

The auction flag is not the draft type: Yahoo sends `draft_type: "live"` for a
live auction and says so only through `is_auction_draft` (`"1"` / `"0"`).

## Sources

- Yahoo Sports developer portal: https://sports.yahoo.com/developer/
- Yahoo OAuth 2.0 auth-code flow: https://developer.yahoo.com/oauth2/guide/flows_authcode/
- yahoo_fantasy_api (draft_results during a draft): https://yahoo-fantasy-api.readthedocs.io/en/latest/yahoo_fantasy_api.html
- League draftresults fields: https://y-fantasy-node-docs.vercel.app/resource/league/draft_results
- yfpy rate-limit thread: https://github.com/uberfastman/yfpy/issues/51
- yfpy README (app registration steps): https://yfpy.uberfastman.com/readme/
