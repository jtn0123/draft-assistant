import { Fragment } from "react";
import type { DraftView } from "../types";
import { useCountdown } from "./useCountdown";
import { usePickLabel } from "../pickFormat";

/**
 * The snake, drawn: every pick from the one on the clock up to and including
 * your next one, so "how many people are in front of me" is something you
 * count rather than read.
 *
 * Picks already in the book (keepers) are drawn struck through — they sit in
 * the order but nobody waits on them, which is why the live count below can
 * be smaller than the number of cells.
 */

/** How far to draw before giving up; a full round is 14 here. */
const MAX_CELLS = 16;

/** Plain snake: odd rounds run 1→N, even rounds run N→1. */
function slotForPick(pick: number, teams: number): number {
  const index = (pick - 1) % teams;
  const round = Math.floor((pick - 1) / teams) + 1;
  return round % 2 === 1 ? index + 1 : teams - index;
}

export function SnakeStrip({ view }: { view: DraftView }) {
  const d = view.draft;
  const remaining = useCountdown(d.pick_deadline);
  const label = usePickLabel(d.teams);
  if (d.status === "complete" || d.teams < 1) return null;

  // The snake, corrected for traded picks: the backend lists every pick
  // that changed hands, so a chip names the manager who will actually make
  // it rather than the one whose slot it started in.
  const ownerSlot = (pick: number) =>
    d.traded_pick_slots[String(pick)] ?? slotForPick(pick, d.teams);

  // Trust the backend's own answer for the pick on the clock. If our snake
  // disagrees — a draft type this does not model — say nothing rather than
  // name the wrong manager.
  if (ownerSlot(d.current_pick) !== d.on_clock_slot) return null;

  const names = new Map(view.rosters.map((r) => [r.slot, r.display_name]));
  const taken = new Set(view.rosters.flatMap((r) => r.players.map((p) => p.pick_no)));
  // Through your *next* pick, not just your first: at a snake turn the two
  // are a few apart, and how few is what decides whether you take two of a
  // position. Stopping at the first one hid exactly that.
  const upcoming = d.my_next_picks.filter((p) => p >= d.current_pick);
  const last = upcoming[1] ?? upcoming[0] ?? d.current_pick + MAX_CELLS - 1;

  const cell = (pick: number) => {
    const slot = ownerSlot(pick);
    return {
      pick,
      name: names.get(slot) ?? `Slot ${slot}`,
      kept: taken.has(pick),
      isMine: slot === d.my_slot,
      onClock: pick === d.current_pick,
    };
  };

  // Two stretches, never more: who stands between you and your next pick,
  // and the turn around the pick after that. Your own picks are always in,
  // whatever the distance — anchoring the second stretch on the far pick and
  // counting back once dropped pick 2 off a strip that started at pick 1.
  const NEAR_MAX = 10;
  const TURN_MAX = 4;
  const wanted = new Set<number>();
  const nearEnd = Math.min(upcoming[0] ?? last, d.current_pick + NEAR_MAX - 1);
  for (let p = d.current_pick; p <= nearEnd; p += 1) wanted.add(p);
  if (upcoming[0] !== undefined) wanted.add(upcoming[0]);
  if (upcoming[1] !== undefined) {
    const turnStart = Math.max(upcoming[1] - (TURN_MAX - 1), (upcoming[0] ?? d.current_pick) + 1);
    for (let p = turnStart; p <= upcoming[1]; p += 1) wanted.add(p);
  }
  const picks = [...wanted].sort((a, b) => a - b).slice(0, MAX_CELLS);
  const cells = picks.map(cell);
  if (cells.length === 0) return null;
  // How many picks each chip skipped over, so a jump is never silent.
  const skipped = new Map<number, number>();
  picks.forEach((pick, i) => {
    const jump = i > 0 ? pick - picks[i - 1] - 1 : 0;
    if (jump > 0) skipped.set(pick, jump);
  });

  const ahead = d.picks_until_mine;
  const waiting = d.status === "pre_draft" && d.current_pick === 1;

  return (
    <div className="snake" aria-label="Pick order">
      <div className="snake-lead">
        <span className="snake-title">Up next</span>
        <span className="snake-count">
          {waiting
            ? "waiting to start"
            : d.is_my_pick
              ? "it's you 🎉"
              : ahead === null
                ? "—"
                : `${ahead} ahead of you`}
        </span>
      </div>
      <ol className="snake-track">
        {cells.map((c) => (
          <Fragment key={c.pick}>
            {skipped.has(c.pick) && (
              <li className="snake-gap" aria-label={`${skipped.get(c.pick)} more picks`}>
                +{skipped.get(c.pick)}
              </li>
            )}
            <li
              className={[
                "snake-cell",
                c.onClock && !waiting ? "on-clock" : "",
                c.isMine ? "mine" : "",
                c.kept ? "kept" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              title={
                c.kept
                  ? `Pick ${label(c.pick)} — already kept`
                  : `Pick ${label(c.pick)} — ${c.name}`
              }
            >
              <span className="snake-pick">{label(c.pick)}</span>
              <span className="snake-name">{c.isMine ? "YOU" : c.name}</span>
              {c.onClock && !waiting && remaining !== null && (
                <span className={`snake-timer${remaining <= 10_000 ? " urgent" : ""}`}>
                  {formatClock(remaining)}
                </span>
              )}
            </li>
          </Fragment>
        ))}
      </ol>
    </div>
  );
}

function formatClock(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(total / 60)}:${(total % 60).toString().padStart(2, "0")}`;
}
