import type { DraftView } from "../types";
import { fmt } from "../format";

/**
 * Where the roster is short before it happens: each week someone is on a
 * bye, who, what the lineup scores that week, and how far below a full
 * week that is. Worst week first. An empty slot is named — that is the
 * week to have a body for.
 */
export function ByeWeeks({ view }: { view: DraftView }) {
  const weeks = view.bye_weeks;
  if (weeks.length === 0) return null;
  return (
    <details className="bye-weeks">
      <summary>
        <h2>Bye weeks</h2>
        <span className="muted">
          worst: week {weeks[0].week}, −{fmt(weeks[0].shortfall, 1)}
          {weeks[0].empty_slots.length > 0 && ` (${weeks[0].empty_slots.join(", ")} empty)`}
        </span>
      </summary>
      <ul className="bye-list" aria-label="Bye weeks">
        {weeks.map((w) => (
          <li key={w.week} className={w.empty_slots.length > 0 ? "empty" : ""}>
            <span className="muted">Wk {w.week}</span>
            <span className="bye-out">{w.out.join(", ")}</span>
            <span className="standings-pts">−{fmt(w.shortfall, 1)}</span>
            <span className="muted">{w.empty_slots.length > 0 ? `${w.empty_slots.join(", ")} empty` : ""}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}
