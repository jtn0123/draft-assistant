import { createServer } from "node:http";

export const leagueId = "1000000000000000001";
export const draftId = "2000000000000000002";
const userId = "3000000000000000003";
const players = {
  "qb-1": { full_name: "Rehearsal Passer", position: "QB", team: null },
  "rb-1": { full_name: "Rehearsal Runner", position: "RB", team: null },
  "wr-1": { full_name: "Rehearsal Catcher", position: "WR", team: null },
};
const projections = Object.entries(players).map(([player_id, player], i) => ({
  player_id,
  player,
  stats: {
    pass_yd: i === 0 ? 4200 : 0,
    pass_td: i === 0 ? 32 : 0,
    rush_yd: i === 1 ? 1200 : 0,
    rush_td: i === 1 ? 10 : 0,
    rec_yd: i === 2 ? 1300 : 0,
    rec_td: i === 2 ? 9 : 0,
    rec: i === 2 ? 95 : 0,
    adp_ppr: i + 1,
  },
}));
export async function startFixture() {
  let picks = [],
    outage = false;
  const requests = [];
  const server = createServer(async (req, res) => {
    const path = new URL(req.url, "http://localhost").pathname;
    requests.push({ path, method: req.method, at: Date.now() });
    const send = (value, status = 200) => {
      res.writeHead(status, { "Content-Type": "application/json" });
      res.end(JSON.stringify(value));
    };
    if (path === "/control" && req.method === "POST") {
      let body = "";
      for await (const chunk of req) body += chunk;
      const next = JSON.parse(body);
      if (next.picks !== undefined) picks = next.picks;
      if (next.outage !== undefined) outage = next.outage;
      return send({ ok: true });
    }
    if (path === "/control/requests") return send(requests);
    if (path === `/v1/user/rehearsal`) return send({ user_id: userId, username: "rehearsal" });
    if (path === `/v1/league/${leagueId}`)
      return send({
        league_id: leagueId,
        name: "Native Rehearsal League",
        season: "2026",
        status: "drafting",
        total_rosters: 2,
        draft_id: draftId,
        roster_positions: ["QB", "RB", "WR", "BN"],
        scoring_settings: {
          rec: 1,
          rush_yd: 0.1,
          rush_td: 6,
          rec_yd: 0.1,
          rec_td: 6,
          pass_yd: 0.04,
          pass_td: 4,
        },
        settings: { start_week: 1 },
      });
    if (path === `/v1/league/${leagueId}/users`)
      return send([{ user_id: userId, display_name: "Rehearsal Manager" }]);
    if (path === `/v1/draft/${draftId}`)
      return send({
        draft_id: draftId,
        status: "drafting",
        type: "snake",
        season: "2026",
        settings: { teams: 2, rounds: 4 },
        draft_order: { [userId]: 1, other: 2 },
      });
    if (path === `/v1/draft/${draftId}/picks`)
      return send(outage ? { error: "rehearsal outage" } : picks, outage ? 503 : 200);
    if (
      path.endsWith("/traded_picks") ||
      path.endsWith("/rosters") ||
      path.includes("/matchups/") ||
      path.includes("/transactions/") ||
      path.endsWith("/winners_bracket")
    )
      return send([]);
    if (path === "/v1/players/nfl") return send(players);
    if (path === "/v1/state/nfl") return send({ season: "2026", week: 1, display_week: 1 });
    if (path === "/projections/nfl/2026") return send(projections);
    if (path.startsWith("/projections/nfl/") || path.startsWith("/scores/nfl/")) return send([]);
    return send({ error: `unhandled rehearsal endpoint ${path}` }, 404);
  });
  server.on("connect", (_req, socket) =>
    socket.end("HTTP/1.1 502 Rehearsal blocks external network\r\n\r\n"),
  );
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}`, requests };
}
