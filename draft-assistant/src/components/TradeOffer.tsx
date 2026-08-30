import type { DraftView, PickPrice, TradeVerdict } from "../types";
import { fmt } from "../format";
import { useOffer, type OfferState, type Picks } from "./useOffer";

/**
 * Price an offer: pick a partner, tick what leaves and what arrives, and
 * see both rosters before and after — season with byes honoured, and this
 * week. The same arithmetic as the trade ideas, for any offer at all.
 */
export function TradeOffer({ view }: { view: DraftView }) {
  const offer = useOffer(view);
  return <OfferForm offer={offer} prices={view.pick_prices} />;
}

/** The form itself, for a caller that owns the state (the trade ideas do). */
export function OfferForm({
  offer,
  prices = [],
}: {
  offer: OfferState;
  prices?: PickPrice[];
}) {
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
        <div className="offer-side">
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
          <PickRow offer={offer} prices={prices} side="give" />
        </div>
        <div className="offer-side">
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
          <PickRow offer={offer} prices={prices} side="get" />
        </div>
      </div>
      <button
        onClick={() => void offer.price()}
        disabled={
          offer.busy ||
          (offer.give.length === 0 &&
            offer.get.length === 0 &&
            offer.picks.give.length === 0 &&
            offer.picks.get.length === 0)
        }
      >
        {offer.busy ? "Pricing…" : "Price it"}
      </button>
      {offer.error && <p className="error">{offer.error}</p>}
      {offer.verdict && <Verdict v={offer.verdict} />}
    </details>
  );
}

/**
 * The rounds this draft has a price for, as one line of chips per side.
 * Last season 34 of this league's 38 trades moved a pick, so an offer form
 * without them prices the wrong deal.
 */
function PickRow({
  offer,
  prices,
  side,
}: {
  offer: OfferState;
  prices: PickPrice[];
  side: keyof Picks;
}) {
  if (prices.length === 0) return null;
  const chosen = offer.picks[side];
  return (
    <div className="pick-row">
      <span className="muted small-text">Picks</span>
      {prices.map((p) => (
        <button
          key={p.round}
          type="button"
          className={`pick-chip${chosen.includes(p.round) ? " on" : ""}`}
          aria-pressed={chosen.includes(p.round)}
          aria-label={`Round ${p.round} pick, worth ${fmt(p.points, 0)} points${
            p.example ? ` — ${p.example} went there` : ""
          }`}
          onClick={() => offer.togglePick(side, p.round)}
        >
          R{p.round}
        </button>
      ))}
    </div>
  );
}

function pickTotal(picks: PickPrice[]): number {
  return picks.reduce((sum, p) => sum + p.points, 0);
}

function Verdict({ v }: { v: TradeVerdict }) {
  const picksIn = pickTotal(v.get_picks);
  const picksOut = pickTotal(v.give_picks);
  const mine = v.my_season_after - v.my_season_before + picksIn - picksOut;
  const theirs =
    v.their_season_after - v.their_season_before + picksOut - picksIn;
  const wk = v.my_week_after - v.my_week_before;
  return (
    <div className="verdict" role="status">
      <p className="strong">
        Me {sign(mine)} · {v.partner_name ?? `slot ${v.partner_slot}`}{" "}
        {sign(theirs)} <span className="muted">season, byes honoured</span>
      </p>
      {(v.give_picks.length > 0 || v.get_picks.length > 0) && (
        <p className="muted small-text">
          Picks: {describePicks(v.get_picks, "in")} ·{" "}
          {describePicks(v.give_picks, "out")}{" "}
          <span className="muted">
            priced off this draft, and they pay next season
          </span>
        </p>
      )}
      <p className="muted small-text">
        Week {v.week}: me {fmt(v.my_week_before, 1)} → {fmt(v.my_week_after, 1)}{" "}
        ({sign(wk, 1)}) · them {fmt(v.their_week_before, 1)} →{" "}
        {fmt(v.their_week_after, 1)}
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

function describePicks(picks: PickPrice[], way: "in" | "out"): string {
  if (picks.length === 0) return `nothing ${way}`;
  const rounds = picks.map((p) => `R${p.round}`).join(", ");
  return `${rounds} ${way} (${sign(pickTotal(picks) * (way === "in" ? 1 : -1))})`;
}

function sign(n: number, digits = 0): string {
  return `${n >= 0 ? "+" : "−"}${fmt(Math.abs(n), digits)}`;
}
