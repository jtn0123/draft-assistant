import { useMemo, useState } from "react";
import type { AvailablePlayer, Position } from "../types";
import { fmt, pct } from "../format";

const POSITIONS: Position[] = ["ALL", "QB", "RB", "WR", "TE", "DEF"];

// ---------- board table ----------

export function Board({
  players,
  onDraft,
}: {
  players: AvailablePlayer[];
  onDraft: (id: string, name: string) => void;
}) {
  const [pos, setPos] = useState<Position>("ALL");
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return players
      .filter((p) => pos === "ALL" || p.position === pos)
      .filter((p) => !q || p.name.toLowerCase().includes(q))
      .slice(0, 200);
  }, [players, pos, query]);

  return (
    <div className="board">
      <div className="board-controls">
        <div className="tabs">
          {POSITIONS.map((p) => (
            <button
              key={p}
              className={p === pos ? "tab active" : "tab"}
              onClick={() => setPos(p)}
            >
              {p}
            </button>
          ))}
        </div>
        <input
          className="search"
          placeholder="Search players…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th className="left">Player</th>
            <th>Pos</th>
            <th>Team</th>
            <th>Bye</th>
            <th title="Season points under your league's exact scoring">Pts</th>
            <th title="Value over replacement">VORP</th>
            <th>Tier</th>
            <th>ADP</th>
            <th title="Chance they survive to your next pick">Surv</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {filtered.map((p) => (
            <tr key={p.player_id} className={p.injury_status ? "injured" : ""}>
              <td className="muted">{p.overall_rank}</td>
              <td className="left name-cell">
                {p.name}
                {p.injury_status && <span className="injury">{p.injury_status}</span>}
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
                <button className="ghost small" onClick={() => onDraft(p.player_id, p.name)}>
                  Draft
                </button>
              </td>
            </tr>
          ))}
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

