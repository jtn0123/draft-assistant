import type { MatchupPreview, Starter } from "../types";
import { fmt } from "../format";

type Row = { slot: string; mine: Starter | null; theirs: Starter | null };

/**
 * Both lineups slot by slot, the higher projection on each row marked. A
 * slot one side has filled and the other has not shows as empty on the
 * short side — that is the row to look at.
 */
export function MatchupTable({ matchup }: { matchup: MatchupPreview }) {
  const rows = pair(matchup.my_starters, matchup.opponent_starters);
  const opponent = matchup.opponent_name ?? `Slot ${matchup.opponent_slot}`;
  return (
    <table className="matchup-table" aria-label="Lineups side by side">
      <thead>
        <tr>
          <th className="slot">Slot</th>
          <th>You</th>
          <th className="num">Pts</th>
          <th>{opponent}</th>
          <th className="num">Pts</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={`${r.slot}-${i}`}>
            <td className="slot">{r.slot}</td>
            <td className={side(r.mine, r.theirs)}>{name(r.mine)}</td>
            <td className="num">{points(r.mine)}</td>
            <td className={side(r.theirs, r.mine)}>{name(r.theirs)}</td>
            <td className="num">{points(r.theirs)}</td>
          </tr>
        ))}
        <tr className="total">
          <td className="slot">Total</td>
          <td />
          <td className="num">{fmt(matchup.my_points, 1)}</td>
          <td />
          <td className="num">{fmt(matchup.opponent_points, 1)}</td>
        </tr>
      </tbody>
    </table>
  );
}

function name(s: Starter | null): string {
  if (!s) return "empty";
  return s.injury ? `${s.name} · ${s.injury}` : s.name;
}

function points(s: Starter | null): string {
  return s ? fmt(s.points, 1) : "–";
}

function side(s: Starter | null, other: Starter | null): string {
  if (!s) return "empty";
  return other === null || s.points > other.points ? "edge" : "";
}

/**
 * Pair the two lineups by slot label: the k-th RB on my side against the
 * k-th RB on theirs. Slots come out in the order of the longer lineup, with
 * any slot only the shorter one has appended, so an empty DEF on one side
 * still gets its row.
 */
function pair(mine: Starter[], theirs: Starter[]): Row[] {
  const [longer, shorter] = mine.length >= theirs.length ? [mine, theirs] : [theirs, mine];
  const order: string[] = [];
  for (const s of [...longer, ...shorter]) if (!order.includes(s.slot)) order.push(s.slot);
  const queue = (xs: Starter[], slot: string) => xs.filter((s) => s.slot === slot);
  const rows: Row[] = [];
  for (const slot of order) {
    const a = queue(mine, slot);
    const b = queue(theirs, slot);
    for (let k = 0; k < Math.max(a.length, b.length); k++) {
      rows.push({ slot, mine: a[k] ?? null, theirs: b[k] ?? null });
    }
  }
  return rows;
}
