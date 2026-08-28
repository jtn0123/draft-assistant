import { useMemo, useState } from "react";
import type { AvailablePlayer, Position } from "../types";
import { fmt, pct } from "../format";

// ---------- board table ----------

type SortKey =
  | "rank"
  | "name"
  | "position"
  | "team"
  | "bye"
  | "points"
  | "vorp"
  | "tier"
  | "adp"
  | "survival";
type Dir = "asc" | "desc";
type Sort = { key: SortKey; dir: Dir };

type Column = { key: SortKey; label: string; title?: string; left?: boolean; first: Dir };

// `first` is the direction a fresh click gives: the useful end of each column.
const COLUMNS: Column[] = [
  { key: "rank", label: "#", title: "Overall rank — the default order", first: "asc" },
  { key: "name", label: "Player", left: true, first: "asc" },
  { key: "position", label: "Pos", first: "asc" },
  { key: "team", label: "Team", first: "asc" },
  { key: "bye", label: "Bye", first: "asc" },
  { key: "points", label: "Pts", title: "Season points under your league's exact scoring", first: "desc" },
  { key: "vorp", label: "VORP", title: "Value over replacement", first: "desc" },
  { key: "tier", label: "Tier", first: "asc" },
  { key: "adp", label: "ADP", title: "Average draft position across Sleeper drafts", first: "asc" },
  { key: "survival", label: "Surv", title: "Chance they survive to your next pick", first: "desc" },
];

const DEFAULT_SORT: Sort = { key: "rank", dir: "asc" };

function value(p: AvailablePlayer, key: SortKey): number | string | null {
  switch (key) {
    case "rank":
      return p.overall_rank;
    case "name":
      return p.name;
    case "position":
      return p.position;
    case "team":
      return p.team;
    case "bye":
      return p.bye_week;
    case "points":
      return p.points;
    case "vorp":
      return p.vorp;
    case "tier":
      return p.tier;
    case "adp":
      return p.adp;
    case "survival":
      return p.survival_next;
  }
}

/** Missing values sort last whichever way; ties fall back to overall rank. */
function compare(a: AvailablePlayer, b: AvailablePlayer, sort: Sort): number {
  const av = value(a, sort.key);
  const bv = value(b, sort.key);
  if (av === null && bv === null) return a.overall_rank - b.overall_rank;
  if (av === null) return 1;
  if (bv === null) return -1;
  let c =
    typeof av === "string" && typeof bv === "string"
      ? av.localeCompare(bv)
      : (av as number) - (bv as number);
  if (sort.dir === "desc") c = -c;
  return c || a.overall_rank - b.overall_rank;
}

// Statuses that mean a player may miss games. Sleeper tags a large share of
// healthy starters `Questionable` through August, so that one is shown muted
// (mirrors `serious_injury` in recommend.rs).
const SERIOUS = new Set(["out", "ir", "pup", "sus", "doubtful", "na", "cov"]);

export function Board({
  players,
  positions,
  onDraft,
  draftOver = false,
}: {
  players: AvailablePlayer[];
  positions: Position[];
  onDraft: (id: string, name: string) => void;
  /** Once every pick is in there is nothing left to draft: don't offer it. */
  draftOver?: boolean;
}) {
  const [pos, setPos] = useState<Position>("ALL");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>(DEFAULT_SORT);

  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    return players
      .filter((p) => pos === "ALL" || p.position === pos)
      .filter((p) => !q || p.name.toLowerCase().includes(q))
      .sort((a, b) => compare(a, b, sort));
  }, [players, pos, query, sort]);
  const filtered = matching.slice(0, 200);

  const toggleSort = (column: Column) =>
    setSort((current) =>
      current.key === column.key
        ? { key: column.key, dir: current.dir === "asc" ? "desc" : "asc" }
        : { key: column.key, dir: column.first },
    );

  return (
    <div className="board">
      <div className="board-controls">
        <div className="tabs" role="group" aria-label="Filter players by position">
          {["ALL", ...positions].map((p) => (
            <button
              key={p}
              className={p === pos ? "tab active" : "tab"}
              onClick={() => setPos(p)}
              aria-pressed={p === pos}
            >
              {p}
            </button>
          ))}
        </div>
        <input
          className="search"
          placeholder="Search players…"
          aria-label="Search players"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <span className="board-count" aria-live="polite">
          {matching.length > 200
            ? `Showing 200 of ${matching.length}`
            : `${matching.length} player${matching.length === 1 ? "" : "s"}`}
        </span>
      </div>
      <table>
        <thead>
          <tr>
            {COLUMNS.map((c) => {
              const active = sort.key === c.key;
              return (
                <th
                  key={c.key}
                  className={c.left ? "left" : undefined}
                  aria-sort={active ? (sort.dir === "asc" ? "ascending" : "descending") : undefined}
                >
                  <button
                    className="sort"
                    title={c.title ?? `Sort by ${c.label}`}
                    onClick={() => toggleSort(c)}
                  >
                    {c.label}
                    {active && <span aria-hidden="true">{sort.dir === "asc" ? " ▲" : " ▼"}</span>}
                  </button>
                </th>
              );
            })}
            <th></th>
          </tr>
        </thead>
        <tbody>
          {filtered.map((p) => (
            <tr key={p.player_id} className={p.injury_status ? "injured" : ""}>
              <td className="muted">{p.overall_rank}</td>
              <td className="left name-cell">
                {p.name}
                {p.injury_status && (
                  <span
                    className={SERIOUS.has(p.injury_status.toLowerCase()) ? "injury" : "injury mild"}
                    title={
                      SERIOUS.has(p.injury_status.toLowerCase())
                        ? "May miss games"
                        : "Preseason tag — usually rest days or a minor knock"
                    }
                  >
                    {p.injury_status}
                  </span>
                )}
              </td>
              <td>
                <span className={`pos-badge pos-${p.position}`}>
                  {p.position}
                  {p.position_rank}
                </span>
              </td>
              <td className="muted">{p.team ?? "–"}</td>
              <td className="muted">{p.bye_week ?? "–"}</td>
              <td className="strong">{fmt(p.points)}</td>
              <td>{fmt(p.vorp)}</td>
              <td>
                <span className={`tier tier-${Math.min(p.tier, 5)}`}>T{p.tier}</span>
              </td>
              <td className="muted">{fmt(p.adp)}</td>
              <td className={surClass(p.survival_next)}>{pct(p.survival_next)}</td>
              <td>
                <button
                  className="ghost small"
                  onClick={() => onDraft(p.player_id, p.name)}
                  disabled={draftOver}
                  title={draftOver ? "The draft is complete" : `Mark ${p.name} as drafted`}
                >
                  Draft
                </button>
              </td>
            </tr>
          ))}
          {filtered.length === 0 && (
            <tr>
              <td className="empty-board" colSpan={11}>
                No matching players
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

function surClass(p: number | null): string {
  if (p === null) return "muted";
  if (p < 0.35) return "surv low";
  if (p < 0.7) return "surv mid";
  return "surv high";
}
