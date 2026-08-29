import type { DraftView } from "../types";
import { fmt } from "../format";

/**
 * Last season, for what it says about the people: who trades, who churns
 * the wire, who spends. Folded by default — it is reference, not news.
 */
export function History({ view }: { view: DraftView }) {
  const h = view.history;
  if (!h || h.managers.length === 0) return null;
  return (
    <details className="history">
      <summary>
        <h2>Last season</h2>
        <span className="muted">
          {h.trades} trades · {h.claims} claims
          {h.bids.count > 0 && ` · winning bid median $${h.bids.median}, top quarter $${h.bids.p75}+`}
        </span>
      </summary>
      <ul className="managers" aria-label="Managers last season">
        {h.managers.map((m) => (
          <li key={m.user_id}>
            <span className="standings-name">{m.display_name ?? "(left the league)"}</span>
            <span className="muted">
              {m.wins}–{m.losses}
            </span>
            <span className="muted">{m.trades} tr</span>
            <span className="muted">{m.moves} mv</span>
            <span className="muted">${m.faab_used}</span>
            <span className="muted standings-pts">{fmt(m.points_for)}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}
