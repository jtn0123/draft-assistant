import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import type { DraftView } from "../types";
import { fmt } from "../format";
import { playerFacts, type PlayerFacts } from "../playerLookup";

const ShowPlayer = createContext<((id: string) => void) | null>(null);

/**
 * Tap any player's name on the season screen for what the app knows about
 * him. The provider draws the card; `PlayerName` makes a name tappable and
 * is plain text anywhere there is no provider (the draft screen, tests).
 */
export function PlayerCardProvider({ view, children }: { view: DraftView; children: ReactNode }) {
  const [id, setId] = useState<string | null>(null);
  const facts = id === null ? null : playerFacts(view, id);
  return (
    <ShowPlayer.Provider value={setId}>
      {children}
      {facts && <Card facts={facts} onClose={() => setId(null)} />}
    </ShowPlayer.Provider>
  );
}

export function PlayerName({ id, children }: { id: string; children: ReactNode }) {
  const show = useContext(ShowPlayer);
  if (!show) return <>{children}</>;
  return (
    <button type="button" className="player-link" onClick={() => show(id)}>
      {children}
    </button>
  );
}

function Card({ facts, onClose }: { facts: PlayerFacts; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const rows: [string, string][] = [];
  rows.push(["Owner", facts.owner ?? "free agent"]);
  if (facts.round !== null) rows.push(["Drafted", `round ${facts.round}${facts.keeper ? " · keeper" : ""}`]);
  if (facts.season !== null) rows.push(["Season", `${fmt(facts.season)} pts`]);
  if (facts.week !== null) rows.push([`Week ${facts.weekNo ?? ""}`.trim(), `${fmt(facts.week, 1)} pts`]);
  if (facts.bye !== null) rows.push(["Bye", `week ${facts.bye}`]);
  if (facts.injury) rows.push(["Injury", facts.injury]);
  if (facts.adp !== null) rows.push(["ADP", fmt(facts.adp, 1)]);
  if (facts.trendingAdds !== null) rows.push(["Adds this week", String(facts.trendingAdds)]);
  return (
    <div className="player-card" role="dialog" aria-label={facts.name}>
      <div className="player-card-head">
        {facts.position && <span className={`pos-badge pos-${facts.position}`}>{facts.position}</span>}
        <strong>{facts.name}</strong>
        {facts.team && <span className="muted">{facts.team}</span>}
        <button className="ghost small" onClick={onClose} aria-label="Close player card">
          ✕
        </button>
      </div>
      <dl>
        {rows.map(([k, v]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
      {facts.weeks && facts.weeks.some((w) => w > 0) && (
        <Sparkline weeks={facts.weeks} current={facts.weekNo} />
      )}
    </div>
  );
}

/**
 * The season's shape, one thin bar a week from a shared baseline: a bye is
 * a gap, the current week is the accent, the rest are quieter. Each bar
 * carries its own tooltip and the whole thing reads as text for a screen
 * reader — the numbers are the label, not the colour.
 */
function Sparkline({ weeks, current }: { weeks: number[]; current: number | null }) {
  const max = Math.max(...weeks, 1);
  const width = 272;
  const height = 44;
  const gap = 2;
  const bar = (width - gap * (weeks.length - 1)) / weeks.length;
  const text = weeks.map((w, i) => `wk ${i + 1} ${w > 0 ? fmt(w, 1) : "bye"}`).join(", ");
  const peak = weeks.indexOf(Math.max(...weeks));
  return (
    <figure className="sparkline" aria-label={`Weekly projection: ${text}`}>
      <figcaption className="muted">
        Weekly projection · peak wk {peak + 1} {fmt(weeks[peak], 1)}
      </figcaption>
      <svg width={width} height={height} role="img" aria-hidden="true">
        {weeks.map((w, i) => {
          const h = w > 0 ? Math.max(2, (w / max) * (height - 4)) : 0;
          return (
            <rect
              key={i}
              x={i * (bar + gap)}
              y={height - h}
              width={bar}
              height={h}
              rx={2}
              className={i + 1 === current ? "bar current" : "bar"}
            >
              <title>{`Week ${i + 1}: ${w > 0 ? fmt(w, 1) : "bye"}`}</title>
            </rect>
          );
        })}
      </svg>
    </figure>
  );
}
