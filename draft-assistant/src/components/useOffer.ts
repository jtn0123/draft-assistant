import { useState } from "react";
import { api } from "../api";
import type { DraftView, TeamRoster, TradeVerdict } from "../types";
import { errorMessage } from "../format";

/** An offer handed in from outside the form — a trade idea, priced on tap. */
export interface Prefill {
  partner: number;
  give: string[];
  get: string[];
}

/** Draft rounds on either side of an offer, by round number. */
export interface Picks {
  give: number[];
  get: number[];
}

export interface OfferState {
  me: TeamRoster | null;
  others: TeamRoster[];
  them: TeamRoster | null;
  give: string[];
  get: string[];
  picks: Picks;
  verdict: TradeVerdict | null;
  error: string | null;
  busy: boolean;
  open: boolean;
  setPartner: (slot: number) => void;
  toggleGive: (id: string) => void;
  toggleGet: (id: string) => void;
  /** Throw a round in, or take it back out, on either side. */
  togglePick: (side: keyof Picks, round: number) => void;
  setOpen: (open: boolean) => void;
  /** Price what is ticked. */
  price: () => Promise<void>;
  /** Set the form to `prefill`, open it, and price it at once. */
  load: (prefill: Prefill) => Promise<void>;
}

/**
 * The state behind "Price an offer", kept out of the form so the trade ideas
 * next to it can fill and price one without a second tap.
 */
export function useOffer(view: DraftView): OfferState {
  const mine = view.draft.my_slot;
  const me = view.rosters.find((r) => r.slot === mine) ?? null;
  const others = view.rosters.filter((r) => r.slot !== mine && r.players.length > 0);
  const [partner, setPartnerSlot] = useState<number>(others[0]?.slot ?? 0);
  const [give, setGive] = useState<string[]>([]);
  const [get, setGet] = useState<string[]>([]);
  const [picks, setPicks] = useState<Picks>({ give: [], get: [] });
  const [verdict, setVerdict] = useState<TradeVerdict | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const them = others.find((r) => r.slot === partner) ?? others[0] ?? null;

  const toggle = <T,>(list: T[], id: T) =>
    list.includes(id) ? list.filter((x) => x !== id) : [...list, id];

  const run = async (slot: number, giving: string[], getting: string[], rounds: Picks) => {
    setBusy(true);
    setError(null);
    try {
      setVerdict(await api.evaluateTrade(slot, giving, getting, rounds.give, rounds.get));
    } catch (e) {
      setVerdict(null);
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return {
    me,
    others,
    them,
    give,
    get,
    picks,
    verdict,
    error,
    busy,
    open,
    setPartner: (slot) => {
      setPartnerSlot(slot);
      setGet([]);
      setPicks({ give: [], get: [] });
      setVerdict(null);
    },
    toggleGive: (id) => setGive((g) => toggle(g, id)),
    toggleGet: (id) => setGet((g) => toggle(g, id)),
    togglePick: (side, round) =>
      setPicks((p) => ({ ...p, [side]: toggle(p[side], round) })),
    setOpen,
    price: () => (them ? run(them.slot, give, get, picks) : Promise.resolve()),
    load: (prefill) => {
      setPartnerSlot(prefill.partner);
      setGive(prefill.give);
      setGet(prefill.get);
      setPicks({ give: [], get: [] });
      setVerdict(null);
      setOpen(true);
      return run(prefill.partner, prefill.give, prefill.get, { give: [], get: [] });
    },
  };
}
