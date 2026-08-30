// The draft cockpit's top strip: round, pick, who is on the clock, and the
// upcoming pick queue.

import { useEffect, useState } from "react";
import type { DraftView } from "../types";
import { clockLabel, pickLabel } from "../format";

/** How many upcoming picks to show before the "+n" expander. */
const COLLAPSED = 4;

/**
 * The pick clock as "0:41", re-rendered every second while a deadline is set.
 * Null when nothing is on the clock, so callers can leave the cell out.
 */
function useClock(deadlineMs: number | null): string | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (deadlineMs === null) return undefined;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [deadlineMs]);
  return clockLabel(deadlineMs, now);
}

export function ClockBanner({ view }: { view: DraftView }) {
  const d = view.draft;
  const preDraft = d.status === "pre_draft" && d.total_picks_made === 0;
  const complete = d.status === "complete";
  const clock = useClock(complete ? null : d.clock_deadline_ms);

  return (
    <div className={d.is_my_pick ? "clock is-mine" : "clock"}>
      <div className="clock-cell">
        <span className="label">Round</span>
        <span className="clock-big">{d.current_round}</span>
      </div>
      <div className="clock-cell">
        <span className="label">Pick</span>
        <span className="clock-big num">{pickLabel(d.current_pick, d.teams)}</span>
      </div>
      <div className="clock-main">
        {complete ? (
          <span className="clock-status">Draft complete</span>
        ) : preDraft ? (
          <span className="clock-status">Draft has not started</span>
        ) : d.is_my_pick ? (
          <span className="clock-status is-you">You are on the clock</span>
        ) : (
          <>
            <span className="clock-status">
              On the clock: {d.on_clock_name ?? `Slot ${d.on_clock_slot}`}
            </span>
            {d.picks_until_mine !== null && (
              <span className="mid clock-sub">
                {d.picks_until_mine} pick{d.picks_until_mine === 1 ? "" : "s"} until you
              </span>
            )}
          </>
        )}
      </div>
      {clock !== null && (
        <div className="clock-cell">
          <span className="label">Clock</span>
          <span className="clock-big num clock-timer">{clock}</span>
        </div>
      )}
      <div className="clock-cell clock-next">
        <span className="label">Your picks</span>
        <span className="clock-next-list num">
          {d.my_next_picks
            .slice(0, 4)
            .map((p) => pickLabel(p, d.teams))
            .join(" · ") || "–"}
        </span>
      </div>
    </div>
  );
}

interface QueueEntry {
  pickNo: number;
  label: string;
  team: string;
  isMine: boolean;
  onClock: boolean;
}

function buildQueue(view: DraftView): QueueEntry[] {
  const d = view.draft;
  const total = d.teams * d.rounds;
  const entries: QueueEntry[] = [];
  for (let pick = d.current_pick; pick <= total && entries.length < 24; pick += 1) {
    const slot = slotForPick(pick, d.teams);
    const roster = view.rosters.find((r) => r.slot === slot);
    entries.push({
      pickNo: pick,
      label: pickLabel(pick, d.teams),
      team: roster?.display_name ?? `Slot ${slot}`,
      isMine: d.my_slot === slot,
      onClock: pick === d.current_pick,
    });
  }
  return entries;
}

/** Snake order: odd rounds run 1..n, even rounds run n..1. */
function slotForPick(pickNo: number, teams: number): number {
  const round = Math.floor((pickNo - 1) / teams) + 1;
  const index = (pickNo - 1) % teams;
  return round % 2 === 1 ? index + 1 : teams - index;
}

export function SnakeStrip({ view }: { view: DraftView }) {
  const [expanded, setExpanded] = useState(false);
  const clock = useClock(view.draft.status === "complete" ? null : view.draft.clock_deadline_ms);
  const queue = buildQueue(view);
  if (queue.length === 0) return null;

  const shown = expanded ? queue : queue.slice(0, COLLAPSED);
  const tail = expanded ? [] : queue.slice(-1);
  const hidden = queue.length - shown.length - tail.length;
  const untilMine = view.draft.picks_until_mine;

  return (
    <div className="snake">
      <div className="snake-lead">
        <span className="label">Up next</span>
        <span className="snake-note">
          {expanded
            ? "through your next picks"
            : untilMine === null
              ? `${queue.length} picks ahead`
              : `${untilMine} ahead of you`}
        </span>
      </div>
      <div className="snake-chips">
        {shown.map((entry) => (
          <QueueChip key={entry.pickNo} entry={entry} clock={clock} />
        ))}
        {hidden > 0 && (
          <button type="button" className="snake-more" onClick={() => setExpanded(true)}>
            +{hidden}
          </button>
        )}
        {expanded && (
          <button type="button" className="snake-more" onClick={() => setExpanded(false)}>
            Collapse
          </button>
        )}
        {tail.map((entry) => (
          <QueueChip key={entry.pickNo} entry={entry} clock={clock} />
        ))}
      </div>
    </div>
  );
}

function QueueChip({ entry, clock }: { entry: QueueEntry; clock: string | null }) {
  const className = entry.isMine
    ? "snake-chip is-mine"
    : entry.onClock
      ? "snake-chip is-on-clock"
      : "snake-chip";
  return (
    <span className={className}>
      <span className="snake-pick num">{entry.label}</span>
      <span className="ellipsis">{entry.isMine ? "YOU" : entry.team}</span>
      {entry.onClock && clock !== null && <span className="snake-clock num">{clock}</span>}
    </span>
  );
}
