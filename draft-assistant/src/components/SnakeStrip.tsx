import { Fragment } from "react";
import type { DraftView } from "../types";
import { useCountdown } from "./useCountdown";

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
  if (d.status === "complete" || d.teams < 1) return null;

  // Trust the backend's own answer for the pick on the clock. If our snake
  // disagrees — a draft type this does not model — say nothing rather than
  // name the wrong manager.
  if (slotForPick(d.current_pick, d.teams) !== d.on_clock_slot) return null;

  const names = new Map(view.rosters.map((r) => [r.slot, r.display_name]));
  const taken = new Set(view.rosters.flatMap((r) => r.players.map((p) => p.pick_no)));
  // Through your *next* pick, not just your first: at a snake turn the two
  // are a few apart, and how few is what decides whether you take two of a
  // position. Stopping at the first one hid exactly that.
  const upcoming = d.my_next_picks.filter((p) => p >= d.current_pick);
  const last = upcoming[1] ?? upcoming[0] ?? d.current_pick + MAX_CELLS - 1;

  const cell = (pick: number) => {
    const slot = slotForPick(pick, d.teams);
    return {
      pick,
      name: names.get(slot) ?? `Slot ${slot}`,
      kept: taken.has(pick),
      isMine: slot === d.my_slot,
      onClock: pick === d.current_pick,
    };
  };

  // Early in a round your pick can be twenty away. Rather than run off the
  // end, keep the near stretch and your own turn, and mark the jump between.
  // Built as a set of pick numbers: the near stretch and the turn overlap
  // whenever it is already your pick, and a duplicated chip means duplicated
  // React keys, which is how a chip from the previous league survived a
  // league switch.
  const wanted = new Set<number>();
  const turnStart = Math.max(d.current_pick, upcoming[0] ?? d.current_pick);
  const turnLength = last - turnStart + 1;
  const headLength = Math.max(1, MAX_CELLS - Math.min(turnLength, MAX_CELLS - 1) - 1);
  for (let p = d.current_pick; p < d.current_pick + headLength && p <= last; p += 1) {
    wanted.add(p);
  }
  for (let p = Math.max(turnStart, last - (MAX_CELLS - 2)); p <= last; p += 1) {
    wanted.add(p);
  }
  const picks = [...wanted].sort((a, b) => a - b).slice(0, MAX_CELLS);
  const cells = picks.map(cell);
  // The one jump in the run, if the middle was skipped.
  const jump = picks.findIndex((p, i) => i > 0 && p - picks[i - 1] > 1);
  const gap = jump > 0 ? picks[jump] - picks[jump - 1] - 1 : 0;
  if (cells.length === 0) return null;
  const gapBefore = gap > 0 ? picks[jump] : null;

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
            {gap > 0 && c.pick === gapBefore && (
              <li className="snake-gap" aria-label={`${gap} more picks`}>
                +{gap}
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
              title={c.kept ? `Pick ${c.pick} — already kept` : `Pick ${c.pick} — ${c.name}`}
            >
              <span className="snake-pick">{c.pick}</span>
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
