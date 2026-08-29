import type { Activity as Move, DraftView } from "../types";
import { fmt } from "../format";
import { TradeOffer } from "./TradeOffer";

/** The league's moves, newest first, and the swaps worth proposing. */
export function Activity({ view }: { view: DraftView }) {
  const moves = view.activity.slice(0, 10);
  const ideas = view.trade_ideas;
  const seasonMode = view.draft.status === "complete";
  if (moves.length === 0 && ideas.length === 0 && !seasonMode) return null;
  return (
    <section className="activity">
      {seasonMode && <TradeOffer view={view} />}
      {ideas.length > 0 && (
        <>
          <h2>Trade ideas</h2>
          <ul className="trade-ideas" aria-label="Trade ideas">
            {ideas.map((i) => (
              <li key={`${i.partner_slot}-${i.give_id}-${i.get_id}`}>
                <span>
                  <strong>{i.get}</strong> <span className="muted">{i.get_position}</span> for{" "}
                  {i.give} <span className="muted">{i.give_position}</span>
                  {i.also_give && (
                    <>
                      {" + "}
                      {i.also_give} <span className="muted">{i.also_give_position}</span>
                    </>
                  )}
                </span>
                <span className="muted">{i.partner_name ?? `slot ${i.partner_slot}`}</span>
                <span className="gain" title={`+${fmt(i.my_gain)} to my lineup; +${fmt(i.over_waiver)} over the best free agent`}>
                  +{fmt(i.over_waiver)} <span className="muted">/ them +{fmt(i.their_gain)}</span>
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {moves.length > 0 && (
        <>
          <h2>League activity</h2>
          <ul className="moves" aria-label="League activity">
            {moves.map((m) => (
              <li key={m.at} className={m.kind}>
                <span className="muted">{when(m.at)}</span>
                <span>{describe(m)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function describe(m: Move): string {
  const who = m.teams.join(" ↔ ");
  if (m.kind === "trade") {
    const bits = [
      ...m.adds.map(([team, p]) => `${p} → ${team}`),
      ...m.picks,
    ];
    return `${who}: ${bits.join(", ") || "trade"}`;
  }
  const add = m.adds.map(([, p]) => p).join(", ");
  const drop = m.drops.map(([, p]) => p).join(", ");
  const bid = m.bid != null ? ` ($${m.bid})` : "";
  const verb = m.kind === "waiver" ? "claimed" : m.kind === "commissioner" ? "commissioner:" : "added";
  const parts = [add && `${verb} ${add}${bid}`, drop && `dropped ${drop}`].filter(Boolean);
  return `${who} ${parts.join(", ")}`;
}

function when(ms: number): string {
  return new Date(ms).toLocaleDateString([], { month: "short", day: "numeric" });
}
