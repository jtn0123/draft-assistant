# Companion API (host ↔ phone / follower desktop)

The desktop app can host a small HTTP + WebSocket server so a phone (trimmed
page) or a second desktop app (full app, follower mode) can watch the same
league and share one chat thread. Off by default. LAN only. Default port 7878.

## Pairing

- Host: Settings → Phone & second screen → Turn on. Shows `http://<ip>:7878/`
  as text and QR, and a 6-digit **pairing code**. The code rotates on Revoke,
  after every successful pairing (a used code is spent), and every 10 minutes
  it has sat unused — whether or not anything is paired, checked by a timer
  the server runs, so a host with Settings closed does not leave the same six
  digits on screen all afternoon. Rotation never invalidates an existing token.
- Turning the connection on is remembered (`companion_enabled` in the app
  config). The host starts the server again at launch on the port it last used,
  so a crash or a restart mid-draft brings the phones back without anyone
  opening Settings.
- Client: `POST /api/pair` body `{ "code": "123456", "device_name": "Rob's iPhone", "kind": "phone" | "desktop", "device_id": "<id>" | null }`
  → `200 { "token": "<opaque>", "host_name": "Justin's Mac", "device_id": "<id>" }` or `403 { "error": "wrong code" }`.
  `device_id` is the id this client was given last time, if it has one: only that
  replaces its old entry (and its old token stops working — any WebSocket still
  open on it is closed with **4401**). **Every client that has paired before must
  send it**, phone page and follower desktop alike; a client that does not is a
  new device each time it pairs and the host's list fills up with copies of it.
  A device pairing under a name that is taken is numbered instead — "iPhone", "iPhone 2" — rather than
  evicting the phone already there.
  Five wrong codes in a minute → `429` for 60 s, counted **per peer address**, so
  one machine guessing does not lock the rest of the house out.
- Pairings and their tokens are persisted owner-only (0600) in the app data
  directory, so restarting the host does not silently unpair every device.
- Every other request: header `Authorization: Bearer <token>`; WebSocket: `GET /api/events?token=<token>`.
  Unknown/revoked token → `401 { "error": "not paired" }`; on the WebSocket the
  handshake is accepted and immediately closed with code **4401** ("revoked"),
  because a browser is told nothing about a refused handshake. A client that sees
  a 4401 close or a 401 from any fetch drops its token and shows the pairing screen.

## Read endpoints (JSON, same shapes the desktop already validates)

- `GET /api/state` → `DraftView` (schema-gated by the client) or `404 { "error": "no league loaded" }`
- `GET /api/season` → `SeasonView` or `404`
- `GET /api/config` → `{ "active_league_id", "leagues": StoredLeague[], "my_user_id", "host_name", "platform" }` — never keys, tokens, or budget
- `GET /api/headshot/{player_id}` → image bytes (`content-type` set) or `404`; `GET /api/avatar/{reference}?full=1` likewise
- `GET /api/devices` → `Device[]` where `Device = { device_id, name, kind, paired_at_ms, last_seen_ms, connected }`
- `GET /` and `/static/*` → the phone page (no token needed for `/`; the page asks for the code).
  Files: `index.html`, `helpers.js`, `clock.js`, `app.js`, `app.css`.

## Response headers

Every response carries `Content-Security-Policy: default-src 'none'; script-src 'self';
style-src 'self'; img-src 'self' data:; connect-src 'self' <ws origins>; base-uri 'none';
form-action 'none'; frame-ancestors 'none'`, plus `X-Content-Type-Options: nosniff` and
`Referrer-Policy: no-referrer`.

`<ws origins>` is this server's own socket origins, spelled out — `ws://127.0.0.1:<port>`,
`ws://localhost:<port>`, `ws://[::1]:<port>`, `ws://<lan-ip>:<port>`, and
`ws://<tailscale-ip>:<port>` when the Mac is on a tailnet — because a browser reads
`connect-src 'self'` as the page's own _scheme_, and `ws://` is not `http://`. The bare
`ws:`/`wss:` schemes are deliberately not there: they admitted a socket to any host.

Reads stay open to any origin (the bearer token is the whole access control), but a
state-changing request that carries an `Origin` must name one of the http origins the
server actually bound (the same list, `http://`), or one of the two follower origins,
`tauri://localhost` and `http://localhost:1420`; anything else is `403`. Another machine
on the same LAN is somebody else's origin, not this server's.

## Shared chat

- Thread per league per screen. `GET /api/chat?screen=draft|season` →
  `SharedChatThread = { league_id, screen, busy: bool, entries: SharedEntry[] }`
  `SharedEntry = { id, at_ms, device: { name, kind }, role: "user" | "assistant", text, cost_usd: number|null, error: string|null }`
  The assistant's entry carries `device` = the device that asked.
- `POST /api/chat` body `{ "screen": "draft", "text": "who should I take?" }` → `202 { "entry_id" }`.
  The question is appended at once; the answer (or an `error` entry) arrives later over WebSocket.
  `409 { "error": "busy" }` while another question is being answered; 10 questions/min/device → `429`.
  Answers use the host's provider (API key or Claude Code) and the host's per-league budget cap.
- `POST /api/chat/reset` body `{ "screen": "draft" }` → `200 SharedChatThread` with no entries.
  Any paired device may start the thread over, and the emptied thread goes out on the WebSocket
  the same way an answer does. A screen that is being answered keeps `busy` until that answer lands.

## WebSocket `/api/events`

Server → client text frames: `{ "type": T, "payload": P }` with
`T ∈ hello | draft-updated (DraftView) | season-updated (SeasonView) | poll-health | season-poll-health | shared-chat (SharedChatThread) | devices (Device[]) | revoked ({})`.
On connect the server sends, in order: `hello` `{ server_now_ms, host_name }`, then
`devices`, the current `draft-updated` and `season-updated` snapshots when a league is
loaded, and the current `shared-chat` for both screens — so a client that reconnects is
up to date without asking for anything.

`hello.server_now_ms` is the host's own clock. A client keeps
`offset = server_now_ms - Date.now()` and adds it wherever it counts a host deadline
down (the pick clock), so a device whose clock is minutes out does not show a pick timer
that is minutes wrong.

Client → server: `{ "type": "ping" }` every 25 s; the server answers `{ "type": "pong" }`.
**Clients must read the pong.** A socket the network has dropped stays `readyState === 1`
for ever: two pings in a row with no `pong` means the connection is gone, and the client
closes it and reconnects with the usual backoff rather than sitting on live-looking stale
data. Clients should also force a reconnect and re-fetch on `visibilitychange` (to
visible), `pageshow` and `online`, which is when a phone wakes with a socket the operating
system has already thrown away.

A socket whose token has been replaced by a re-pair is closed with **4401**, the same code
an unknown token gets, so the client goes back to the pairing screen instead of reading on
with a token the host has forgotten.

## Desktop (Tauri) commands on the host

- `companion_status()` → `{ enabled, url, tailscale_url, code, port, host_name, devices: Device[] }` — `tailscale_url` is the same server on the Mac's `100.64.0.0/10` address when it has one, so a phone on the tailnet can pair from anywhere
- `companion_enable()` → status (also writes `companion_enabled = true`, so the next
  launch starts the server on its own) · `companion_disable()` → status (clears the flag) · `companion_revoke()` → status (new code, every client dropped with `revoked`)
- `set_device_name(name)` → `String` (the host's own name in shared chat; default = the Mac's computer name)
- `shared_chat_get(screen)` → `SharedChatThread` · `shared_chat_send(screen, text)` → `()` (attributed to the host device)
- Webview events: `shared-chat` (thread), `companion-devices` (Device[]). The code
  rotates behind the panel (after a pairing, and every 10 minutes it sits unused) and
  each rotation emits `companion-devices`, so the panel should re-read
  `companion_status()` on that event rather than only replacing its device list.

## Follower desktop

Settings → "Join another Draft Assistant…" (also on the first-launch screen): enter `host:port` (or the URL from the QR) and the code.
The app then runs its `Api` against the host over HTTP/WS; league switching, keys, budget, Yahoo and username edits are host-only and disabled with a "Hosted by Justin's Mac" pill. "Leave" returns to local mode. Stored in `localStorage` (`da.companion.follow` = `{ url, token, host_name }`).
