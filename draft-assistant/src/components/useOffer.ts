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

export interface OfferState {
  me: TeamRoster | null;
  others: TeamRoster[];
  them: TeamRoster | null;
  give: string[];
  get: string[];
  verdict: TradeVerdict | null;
  error: string | null;
  busy: boolean;
  open: boolean;
  setPartner: (slot: number) => void;
  toggleGive: (id: string) => void;
  toggleGet: (id: string) => void;
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
  const [verdict, setVerdict] = useState<TradeVerdict | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const them = others.find((r) => r.slot === partner) ?? others[0] ?? null;

  const toggle = (list: string[], id: string) =>
    list.includes(id) ? list.filter((x) => x !== id) : [...list, id];

  const run = async (slot: number, giving: string[], getting: string[]) => {
    setBusy(true);
    setError(null);
    try {
      setVerdict(await api.evaluateTrade(slot, giving, getting));
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
    verdict,
    error,
    busy,
    open,
    setPartner: (slot) => {
      setPartnerSlot(slot);
      setGet([]);
      setVerdict(null);
    },
    toggleGive: (id) => setGive((g) => toggle(g, id)),
    toggleGet: (id) => setGet((g) => toggle(g, id)),
    setOpen,
    price: () => (them ? run(them.slot, give, get) : Promise.resolve()),
    load: (prefill) => {
      setPartnerSlot(prefill.partner);
      setGive(prefill.give);
      setGet(prefill.get);
      setVerdict(null);
      setOpen(true);
      return run(prefill.partner, prefill.give, prefill.get);
    },
  };
}
