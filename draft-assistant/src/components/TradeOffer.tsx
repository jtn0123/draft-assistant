import { useState } from "react";
import { api } from "../api";
import type { DraftView, TradeVerdict } from "../types";
import { errorMessage, fmt } from "../format";

/**
 * Price an offer: pick a partner, tick what leaves and what arrives, and
 * see both rosters before and after — season with byes honoured, and this
 * week. The same arithmetic as the trade ideas, for any offer at all.
 */
export function TradeOffer({ view }: { view: DraftView }) {
  const mine = view.draft.my_slot;
  const me = view.rosters.find((r) => r.slot === mine);
  const others = view.rosters.filter((r) => r.slot !== mine && r.players.length > 0);
  const [partner, setPartner] = useState<number>(others[0]?.slot ?? 0);
  const [give, setGive] = useState<string[]>([]);
  const [get, setGet] = useState<string[]>([]);
  const [verdict, setVerdict] = useState<TradeVerdict | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  if (!me || others.length === 0 || mine === null) return null;
  const them = others.find((r) => r.slot === partner) ?? others[0];
  const toggle = (list: string[], id: string) =>
    list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
  const price = async () => {
    setBusy(true);
    setError(null);
    try {
      setVerdict(await api.evaluateTrade(them.slot, give, get));
    } catch (e) {
      setVerdict(null);
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <details className="trade-offer">
      <summary>
        <h2>Price an offer</h2>
      </summary>
      <label className="muted small-text">
        With{" "}
        <select
          aria-label="Trade partner"
          value={them.slot}
          onChange={(e) => {
            setPartner(Number(e.target.value));
            setGet([]);
            setVerdict(null);
          }}
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
                checked={give.includes(p.player_id)}
                onChange={() => setGive((g) => toggle(g, p.player_id))}
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
                checked={get.includes(p.player_id)}
                onChange={() => setGet((g) => toggle(g, p.player_id))}
              />{" "}
              <span className="muted">{p.position}</span> {p.name}
            </label>
          ))}
        </fieldset>
      </div>
      <button onClick={() => void price()} disabled={busy || (give.length === 0 && get.length === 0)}>
        {busy ? "Pricing…" : "Price it"}
      </button>
      {error && <p className="error">{error}</p>}
      {verdict && <Verdict v={verdict} />}
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
