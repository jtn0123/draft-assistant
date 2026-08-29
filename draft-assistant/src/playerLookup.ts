import type { DraftView, Starter } from "./types";

/** Everything the view knows about one player, gathered for a tap. */
export interface PlayerFacts {
  player_id: string;
  name: string;
  position: string | null;
  team: string | null;
  /** "YOU", a manager's name, or null for a free agent. */
  owner: string | null;
  round: number | null;
  keeper: boolean;
  /** Season projection under league scoring. */
  season: number | null;
  /** This week's projection, and which week. */
  week: number | null;
  weekNo: number | null;
  injury: string | null;
  bye: number | null;
  adp: number | null;
  trendingAdds: number | null;
  /** Projected points, weeks 1..17, when the view carries them. */
  weeks: number[] | null;
}

/**
 * Assemble what is known about `id` from wherever the view carries it: the
 * rosters (owner, round), the standings' lineups (season and week points),
 * this week's matchup, the waiver board, or the free-agent board. Null if
 * the id is nowhere.
 */
export function playerFacts(view: DraftView, id: string): PlayerFacts | null {
  const facts: PlayerFacts = {
    player_id: id,
    name: "",
    position: null,
    team: null,
    owner: null,
    round: null,
    keeper: false,
    season: null,
    week: null,
    weekNo: null,
    injury: null,
    bye: null,
    adp: null,
    trendingAdds: null,
    weeks: view.player_weeks[id] ?? null,
  };
  let found = false;
  const mine = view.draft.my_slot;

  for (const r of view.rosters) {
    const p = r.players.find((x) => x.player_id === id);
    if (!p) continue;
    found = true;
    facts.name = p.name;
    facts.position = p.position;
    facts.team = p.team;
    facts.owner = r.slot === mine ? "YOU" : (r.display_name ?? `Slot ${r.slot}`);
    facts.round = p.round;
    facts.keeper = p.is_keeper;
  }

  const starter = (s: Starter, kind: "season" | "week", weekNo: number) => {
    found = true;
    facts.name ||= s.name;
    facts.position ??= s.position;
    if (kind === "season") facts.season ??= s.points;
    else {
      facts.week ??= s.points;
      facts.weekNo ??= weekNo;
    }
    facts.injury ??= s.injury;
  };
  for (const t of view.projected_standings) {
    for (const s of t.starters) if (s.player_id === id) starter(s, "season", t.week);
    for (const s of t.week_starters) if (s.player_id === id) starter(s, "week", t.week);
  }
  const w = view.this_week;
  if (w) {
    for (const s of [...(w.matchup?.my_starters ?? []), ...(w.matchup?.opponent_starters ?? [])]) {
      if (s.player_id === id) starter(s, "week", w.week);
    }
    for (const c of w.lineup?.changes ?? []) {
      if (c.in_.player_id === id) starter(c.in_, "week", w.week);
      if (c.out?.player_id === id) starter(c.out, "week", w.week);
    }
  }

  const target = view.waivers?.targets.find((t) => t.player_id === id);
  if (target) {
    found = true;
    facts.name ||= target.name;
    facts.position ??= target.position;
    facts.team ??= target.team;
    facts.season ??= target.points;
    facts.bye ??= target.bye_week;
    facts.trendingAdds = target.trending_adds;
  }
  const free = view.available.find((p) => p.player_id === id);
  if (free) {
    found = true;
    facts.name ||= free.name;
    facts.position ??= free.position;
    facts.team ??= free.team;
    facts.season ??= free.points;
    facts.bye ??= free.bye_week;
    facts.injury ??= free.injury_status;
    facts.adp = free.adp;
  }
  if (facts.bye === null && facts.owner === "YOU") {
    facts.bye = view.bye_weeks.find((b) => b.out.includes(facts.name))?.week ?? null;
  }
  return found ? facts : null;
}
