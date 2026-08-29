import type { DraftView, TradeVerdict } from "../types";
import { fmt } from "../format";
import { useOffer, type OfferState } from "./useOffer";

/**
 * Price an offer: pick a partner, tick what leaves and what arrives, and
 * see both rosters before and after — season with byes honoured, and this
 * week. The same arithmetic as the trade ideas, for any offer at all.
 */
export function TradeOffer({ view }: { view: DraftView }) {
  const offer = useOffer(view);
  return <OfferForm offer={offer} />;
}

/** The form itself, for a caller that owns the state (the trade ideas do). */
export function OfferForm({ offer }: { offer: OfferState }) {
  const { me, others, them } = offer;
  if (!me || !them || others.length === 0) return null;
  return (
    <details
      className="trade-offer"
      open={offer.open}
      onToggle={(e) => offer.setOpen(e.currentTarget.open)}
    >
      <summary>
        <h2>Price an offer</h2>
      </summary>
      <label className="muted small-text">
        With{" "}
        <select
          aria-label="Trade partner"
          value={them.slot}
          onChange={(e) => offer.setPartner(Number(e.target.value))}
        >
          {others.map((r) => (
            <option key={r.slot} value={r.slot}>
              {r.display_name ?? `Slot ${r.slot}`}
            </option>
          ))}
        </select>
      </label>
      <div className="offer-sides">
        <fieldset>
          <legend>I give</legend>
          {me.players.map((p) => (
            <label key={p.player_id}>
              <input
                type="checkbox"
                checked={offer.give.includes(p.player_id)}
                onChange={() => offer.toggleGive(p.player_id)}
              />{" "}
              <span className="muted">{p.position}</span> {p.name}
            </label>
          ))}
        </fieldset>
        <fieldset>
          <legend>I get</legend>
          {them.players.map((p) => (
            <label key={p.player_id}>
              <input
                type="checkbox"
                checked={offer.get.includes(p.player_id)}
                onChange={() => offer.toggleGet(p.player_id)}
              />{" "}
              <span className="muted">{p.position}</span> {p.name}
            </label>
          ))}
        </fieldset>
      </div>
      <button
        onClick={() => void offer.price()}
        disabled={offer.busy || (offer.give.length === 0 && offer.get.length === 0)}
      >
        {offer.busy ? "Pricing…" : "Price it"}
      </button>
      {offer.error && <p className="error">{offer.error}</p>}
      {offer.verdict && <Verdict v={offer.verdict} />}
    </details>
  );
}

function Verdict({ v }: { v: TradeVerdict }) {
  const mine = v.my_season_after - v.my_season_before;
  const theirs = v.their_season_after - v.their_season_before;
  const wk = v.my_week_after - v.my_week_before;
  return (
    <div className="verdict" role="status">
      <p className="strong">
        Me {sign(mine)} · {v.partner_name ?? `slot ${v.partner_slot}`} {sign(theirs)}{" "}
        <span className="muted">season, byes honoured</span>
      </p>
      <p className="muted small-text">
        Week {v.week}: me {fmt(v.my_week_before, 1)} → {fmt(v.my_week_after, 1)} ({sign(wk, 1)}) ·
        them {fmt(v.their_week_before, 1)} → {fmt(v.their_week_after, 1)}
      </p>
      <p className="muted small-text">
        {theirs < 0
          ? "They lose on it — expect a no."
          : mine < 0
            ? "You lose on it."
            : "Both sides gain — worth sending."}
      </p>
    </div>
  );
}

function sign(n: number, digits = 0): string {
  return `${n >= 0 ? "+" : "−"}${fmt(Math.abs(n), digits)}`;
}
