#!/usr/bin/env node
// Replay a completed Sleeper draft as if it were happening now.
//
// Stands in for api.sleeper.app on a local port: the league, draft, and picks
// endpoints for one draft are served from a recording, with picks released on
// a timer; everything else (players, projections, users, scores) is proxied to
// the real API. Point a debug build at it with DRAFT_ASSISTANT_SLEEPER_BASE,
// or let it write state dumps and watch the browser preview follow along via
// `?replay=/live-state.json`.
//
//   node scripts/replay-sleeper.mjs --league <id> --draft <id> [options]
//
//   --port 8787         listen port
//   --interval 8        seconds between released picks
//   --start-at 0        picks already made when the server starts
//   --season 2026       rewrite the league/draft season so current projections load
//   --my-user <id>      put this Sleeper user at --my-slot (replacing whoever drafted there)
//   --my-slot 2
//   --dump <path>       write dump_state output here every --dump-every seconds
//   --dump-season <path>  the same, for dump_season (the preview's ?replay-season)
//   --dump-every 4
//   --username <name>   Sleeper username handed to the dump binaries ("my team")
//   --bin-dir <path>    where dump_state/dump_season live
//                       (default: src-tauri/target/debug)
//
// The dump binaries are debug builds on purpose: only a debug build honours
// DRAFT_ASSISTANT_SLEEPER_BASE, so a shipped app can never be pointed here.
// Build them with:
//   cargo build --manifest-path src-tauri/Cargo.toml --bin dump_state --bin dump_season
//
// Control while running:  GET /replay/status | /replay/step | /replay/pause |
//   /replay/resume | /replay/set?n=<picks>

import http from "node:http";
import { spawn } from "node:child_process";
import { parseArgs } from "node:util";
import { resolve } from "node:path";

const UPSTREAM = "https://api.sleeper.app";

const { values: o } = parseArgs({
  options: {
    league: { type: "string" },
    draft: { type: "string" },
    port: { type: "string", default: "8787" },
    interval: { type: "string", default: "8" },
    "start-at": { type: "string", default: "0" },
    season: { type: "string" },
    "my-user": { type: "string" },
    "my-slot": { type: "string" },
    dump: { type: "string" },
    "dump-season": { type: "string" },
    "dump-every": { type: "string", default: "4" },
    username: { type: "string" },
    "bin-dir": { type: "string", default: "src-tauri/target/debug" },
  },
});

if (!o.league || !o.draft) {
  console.error(
    "usage: replay-sleeper.mjs --league <id> --draft <id> [--port N] [--interval S] ...",
  );
  process.exit(2);
}

const port = Number(o.port);
const interval = Number(o.interval);
const log = (...args) => console.log(new Date().toISOString().slice(11, 19), ...args);

async function upstream(path) {
  const resp = await fetch(UPSTREAM + path, {
    headers: { "user-agent": "draft-assistant-replay" },
  });
  if (!resp.ok) throw new Error(`${path}: HTTP ${resp.status}`);
  return resp.json();
}

// ---- recording ----
const league = await upstream(`/v1/league/${o.league}`);
const draft = await upstream(`/v1/draft/${o.draft}`);
const picks = (await upstream(`/v1/draft/${o.draft}/picks`)).sort((a, b) => a.pick_no - b.pick_no);
// Pick trades are fixed before the draft, so the whole list is served from the
// start; without it the app reads the order as a plain snake and warns.
const traded = await upstream(`/v1/draft/${o.draft}/traded_picks`).catch(() => []);
if (picks.length === 0) {
  console.error("that draft has no picks to replay");
  process.exit(1);
}
const total = picks.length;
log(
  `recorded ${league.name}: ${draft.settings.teams} teams × ${draft.settings.rounds} rounds, ` +
    `${total} picks, status ${draft.status}`,
);

if (o.season) {
  league.season = o.season;
  draft.season = o.season;
}
if (o["my-user"] && o["my-slot"]) {
  const slot = Number(o["my-slot"]);
  const order = { ...(draft.draft_order ?? {}) };
  for (const [user, s] of Object.entries(order)) {
    if (s === slot || user === o["my-user"]) delete order[user];
  }
  order[o["my-user"]] = slot;
  draft.draft_order = order;
  for (const pick of picks) {
    if (pick.draft_slot === slot) pick.picked_by = o["my-user"];
  }
  log(`user ${o["my-user"]} placed at slot ${slot}`);
}
league.draft_id = o.draft;
league.status = "drafting";

// ---- clock ----
const serverStart = Date.now();
let released = Math.min(Number(o["start-at"]), total);
let paused = false;
let lastPickedAt = null;

function describe(pick) {
  const m = pick.metadata ?? {};
  return `${m.first_name ?? ""} ${m.last_name ?? pick.player_id} ${m.position ?? ""}`.trim();
}

function release(n) {
  const before = released;
  released = Math.max(0, Math.min(n, total));
  if (released !== before) lastPickedAt = Date.now();
  for (let i = before; i < released; i += 1) {
    const p = picks[i];
    log(`pick ${p.pick_no} (R${p.round} slot ${p.draft_slot}): ${describe(p)}`);
  }
  if (released === total) log("draft complete");
}

setInterval(() => {
  if (!paused && released < total) release(released + 1);
}, interval * 1000);

function currentDraft() {
  return {
    ...draft,
    status: released >= total ? "complete" : released > 0 ? "drafting" : "pre_draft",
    start_time: serverStart,
    // Stamped when the pick was actually released, so the app's pick clock
    // (last_picked + pick_timer) counts down like a real draft.
    last_picked: released > 0 ? (lastPickedAt ?? serverStart) : null,
  };
}

// ---- server ----
function send(res, status, body, type = "application/json") {
  res.writeHead(status, { "content-type": type, "access-control-allow-origin": "*" });
  res.end(body);
}

/** The replay's own control surface; null when the path is not one of them. */
function control(path, url) {
  if (path === "/replay/status") {
    return {
      released,
      total,
      interval,
      paused,
      uptime_s: Math.round((Date.now() - serverStart) / 1000),
    };
  }
  if (path === "/replay/step") {
    release(released + 1);
    return { released, total };
  }
  if (path === "/replay/pause" || path === "/replay/resume") {
    paused = path.endsWith("pause");
    log(paused ? "paused" : "resumed");
    return { paused, released, total };
  }
  if (path === "/replay/set") {
    release(Number(url.searchParams.get("n") ?? released));
    return { released, total };
  }
  return null;
}

/** The recorded endpoints; null when the path belongs to the real API. */
function recorded(path, who) {
  if (path === `/v1/league/${o.league}`) {
    log(`${who} league`);
    return league;
  }
  if (path === `/v1/draft/${o.draft}`) return currentDraft();
  if (path === `/v1/draft/${o.draft}/picks`) {
    log(`${who} picks -> ${released}/${total}`);
    return picks.slice(0, released);
  }
  if (path === `/v1/draft/${o.draft}/traded_picks`) return traded ?? [];
  return null;
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://localhost:${port}`);
  const path = url.pathname;
  const who = req.socket.remotePort;
  try {
    const answer = recorded(path, who) ?? control(path, url);
    if (answer !== null) return send(res, 200, JSON.stringify(answer));
    // Everything else is the real API.
    const resp = await fetch(UPSTREAM + path + url.search, {
      headers: { "user-agent": "draft-assistant-replay" },
    });
    const body = Buffer.from(await resp.arrayBuffer());
    log(`${who} proxy ${path} -> ${resp.status} (${body.length} bytes)`);
    return send(res, resp.status, body, resp.headers.get("content-type") ?? "application/json");
  } catch (error) {
    log(`error serving ${path}: ${error.message}`);
    return send(res, 502, JSON.stringify({ error: String(error.message) }));
  }
});

server.listen(port, "127.0.0.1", () => {
  log(
    `replay server on http://localhost:${port} — ${released}/${total} picks released, ` +
      `one every ${interval}s`,
  );
  log(`DRAFT_ASSISTANT_SLEEPER_BASE=http://localhost:${port}`);
});

// ---- optional state dumps for the browser preview ----

/**
 * Re-run one dump binary against this server, forever. Each run rewrites the
 * whole file, and the preview orders the results by `generated_at` — so a
 * half-written file it catches mid-run is simply skipped.
 */
function keepDumping(binary, out) {
  const bin = resolve(o["bin-dir"], binary);
  const path = resolve(out);
  const every = Number(o["dump-every"]) * 1000;
  log(`${binary} -> ${path} every ${every / 1000}s`);
  const run = () => {
    const args = [o.league];
    if (o.username) args.push(o.username);
    args.push(path);
    const child = spawn(bin, args, {
      env: { ...process.env, DRAFT_ASSISTANT_SLEEPER_BASE: `http://localhost:${port}` },
      stdio: ["ignore", "ignore", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (d) => (stderr += d));
    child.on("exit", (code) => {
      if (code !== 0) log(`${binary} exited ${code}: ${stderr.trim().split("\n").pop()}`);
      setTimeout(run, every);
    });
    child.on("error", (e) => {
      log(`${binary} failed to start: ${e.message} (build it first — see the header)`);
      setTimeout(run, every * 5);
    });
  };
  run();
}

if (o.dump) keepDumping("dump_state", o.dump);
if (o["dump-season"]) keepDumping("dump_season", o["dump-season"]);
