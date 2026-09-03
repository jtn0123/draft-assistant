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
- Flow: open `https://api.login.yahoo.com/oauth2/request_auth` in the browser,
  redirect to `https://localhost:<port>` (a valid https localhost URI is
  required at registration; `oob` also works and shows a code to paste), swap
  the code at `/oauth2/get_token`. Access tokens last 1 hour; refresh tokens
  do not expire (they survive password changes) — store the refresh token in
  the Keychain next to the Anthropic key via `secrets.rs`.
- Rate limits are undocumented; Yahoo "may throttle excessive use". Wrappers
  report failures only on many-thousands-of-calls backfills, so a 3-second
  draft poll is fine.
- Terms: one developer account, no reverse engineering, attribution line
  "Fantasy data provided by Yahoo Fantasy" somewhere in the UI.

## What it would take here

1. `yahoo.rs` client: token store + refresh, `get_json` with the Bearer header,
   the three loaders (league+settings, teams/rosters, draftresults).
2. A `Platform` enum on `StoredLeague` (`sleeper` | `yahoo`) so the picker and
   config carry both; `add_league` dispatches on it. Yahoo keys look like
   `449.l.12345`, so the paste box can tell them apart from Sleeper's digits.
3. Settings → "Connect Yahoo" (opens the browser, catches the redirect on a
   localhost listener, saves the refresh token).
4. Id crosswalk Yahoo player → Sleeper player for projections and headshots.
5. Season screen: matchups/scoreboard/transactions exist, so Season could
   follow later; the draft screen is the first deliverable.

Estimate: 2–3 days for the draft screen, mostly the OAuth plumbing and the
crosswalk. Nothing in the board, recommender or chat needs to change.

## Sources

- Yahoo Sports developer portal: https://sports.yahoo.com/developer/
- Yahoo OAuth 2.0 auth-code flow: https://developer.yahoo.com/oauth2/guide/flows_authcode/
- yahoo_fantasy_api (draft_results during a draft): https://yahoo-fantasy-api.readthedocs.io/en/latest/yahoo_fantasy_api.html
- League draftresults fields: https://y-fantasy-node-docs.vercel.app/resource/league/draft_results
- yfpy rate-limit thread: https://github.com/uberfastman/yfpy/issues/51
- yfpy README (app registration steps): https://yfpy.uberfastman.com/readme/
