# Companion API (host ↔ phone / follower desktop)

The desktop app can host a small HTTP + WebSocket server so a phone (trimmed
page) or a second desktop app (full app, follower mode) can watch the same
league and share one chat thread. Off by default. LAN only. Default port 7878.

## Pairing
- Host: Settings → Phone & second screen → Turn on. Shows `http://<ip>:7878/`
  as text and QR, and a 6-digit **pairing code** (rotates on Revoke / re-enable).
- Client: `POST /api/pair` body `{ "code": "123456", "device_name": "Rob's iPhone", "kind": "phone" | "desktop" }`
  → `200 { "token": "<opaque>", "host_name": "Justin's Mac", "device_id": "<id>" }` or `403 { "error": "wrong code" }`.
  Five wrong codes in a minute → `429` for 60 s.
- Every other request: header `Authorization: Bearer <token>`; WebSocket: `GET /api/events?token=<token>`.
  Unknown/revoked token → `401 { "error": "not paired" }`.

## Read endpoints (JSON, same shapes the desktop already validates)
- `GET /api/state` → `DraftView` (schema-gated by the client) or `404 { "error": "no league loaded" }`
- `GET /api/season` → `SeasonView` or `404`
- `GET /api/config` → `{ "active_league_id", "leagues": StoredLeague[], "my_user_id", "host_name", "platform" }` — never keys, tokens, or budget
- `GET /api/headshot/{player_id}` → image bytes (`content-type` set) or `404`; `GET /api/avatar/{reference}?full=1` likewise
- `GET /api/devices` → `Device[]` where `Device = { device_id, name, kind, paired_at_ms, last_seen_ms, connected }`
- `GET /` and `/static/*` → the phone page (no token needed for `/`; the page asks for the code)

## Shared chat
- Thread per league per screen. `GET /api/chat?screen=draft|season` →
  `SharedChatThread = { league_id, screen, busy: bool, entries: SharedEntry[] }`
  `SharedEntry = { id, at_ms, device: { name, kind }, role: "user" | "assistant", text, cost_usd: number|null, error: string|null }`
  The assistant's entry carries `device` = the device that asked.
- `POST /api/chat` body `{ "screen": "draft", "text": "who should I take?" }` → `202 { "entry_id" }`.
  The question is appended at once; the answer (or an `error` entry) arrives later over WebSocket.
  `409 { "error": "busy" }` while another question is being answered; 10 questions/min/device → `429`.
  Answers use the host's provider (API key or Claude Code) and the host's per-league budget cap.

## WebSocket `/api/events`
Server → client text frames: `{ "type": T, "payload": P }` with
`T ∈ draft-updated (DraftView) | season-updated (SeasonView) | poll-health | season-poll-health | shared-chat (SharedChatThread) | devices (Device[]) | revoked ({})`.
On connect the server sends the current `shared-chat` for both screens and `devices`. Client → server: `{ "type": "ping" }` every 25 s; the server answers `{ "type": "pong" }`.

## Desktop (Tauri) commands on the host
- `companion_status()` → `{ enabled, url, tailscale_url, code, port, host_name, devices: Device[] }` — `tailscale_url` is the same server on the Mac's `100.64.0.0/10` address when it has one, so a phone on the tailnet can pair from anywhere
- `companion_enable()` → status · `companion_disable()` → status · `companion_revoke()` → status (new code, every client dropped with `revoked`)
- `set_device_name(name)` → `String` (the host's own name in shared chat; default = the Mac's computer name)
- `shared_chat_get(screen)` → `SharedChatThread` · `shared_chat_send(screen, text)` → `()` (attributed to the host device)
- Webview events: `shared-chat` (thread), `companion-devices` (Device[])

## Follower desktop
Settings → "Join another Draft Assistant…" (also on the first-launch screen): enter `host:port` (or the URL from the QR) and the code.
The app then runs its `Api` against the host over HTTP/WS; league switching, keys, budget, Yahoo and username edits are host-only and disabled with a "Hosted by Justin's Mac" pill. "Leave" returns to local mode. Stored in `localStorage` (`da.companion.follow` = `{ url, token, host_name }`).
