// The draft cockpit's top strip: round, pick, who is on the clock, and the
// upcoming pick queue.

import { useMemo, useState } from "react";
import type { DraftView, Platform, TeamRoster } from "../types";
import { useNow } from "../clock";
import { clockLabel, pickLabel, spanLabel } from "../format";

/** How many upcoming picks to show before the "+n" expander. */
const COLLAPSED = 4;

/** How far ahead the queue is built. Past this it is truncated, and the strip
 *  has to say so rather than passing the cap off as the whole story. */
const QUEUE_MAX = 24;

/**
 * The pick clock as "0:41", re-rendered every second while a deadline is set.
 * Null when nothing is on the clock, so callers can leave the cell out.
 */
function useClock(deadlineMs: number | null): string | null {
  return clockLabel(deadlineMs, useNow(deadlineMs !== null));
}

/** Whole seconds left on the clock, or null when nothing is running. */
function secondsLeft(deadlineMs: number | null, nowMs: number): number | null {
  if (deadlineMs === null) return null;
  return Math.max(0, Math.ceil((deadlineMs - nowMs) / 1000));
}

/**
 * One sentence saying who is picking, for people who cannot see the banner.
 *
 * It repeats the round, the pick and the time left rather than relying on the
 * green highlight, because the highlight is the only thing that currently
 * says "this one is yours".
 */
function clockSentence(d: DraftView["draft"], left: number | null): string {
  if (d.status === "complete") return "The draft is finished.";
  if (d.status === "pre_draft" && d.total_picks_made === 0) {
    return "The draft has not started yet.";
  }
  // A paused draft still names a manager and a pick, and both are stale: the
  // timer is stopped and nobody can act. Saying whose turn it is invited
  // people to wonder why that manager was taking so long.
  if (d.paused) return "The draft is paused.";
  const pick = `pick ${pickLabel(d.current_pick, d.teams)}`;
  const time = left === null ? "" : `, ${spanLabel(left)} left`;
  if (d.is_my_pick) return `You are on the clock — ${pick}${time}.`;
  const who = d.on_clock_name ?? `slot ${d.on_clock_slot}`;
  const wait =
    d.picks_until_mine === null
      ? ""
      : ` ${d.picks_until_mine} pick${d.picks_until_mine === 1 ? "" : "s"} until your turn.`;
  return `${who} is on the clock — ${pick}${time}.${wait}`;
}

/**
 * Hold a sentence steady until the situation itself changes.
 *
 * The banner re-renders once a second while the clock runs. A live region
 * rebuilt on every one of those renders would interrupt a screen reader every
 * second to say almost exactly the same thing, which is worse than saying
 * nothing at all. So the sentence is captured only when `key` changes — a new
 * pick, the turn changing hands, the draft starting or finishing — and the
 * wording taken at that moment is what the region keeps until the next
 * change. The seconds are read once, on the way in, and then left alone.
 */
function useHeldSentence(key: string, sentence: string): string {
  const [held, setHeld] = useState({ key, sentence });
  if (held.key !== key) setHeld({ key, sentence });
  return held.key === key ? held.sentence : sentence;
}

/**
 * What to write where the clock would be, when there is no clock.
 *
 * Only Yahoo gets a line: a Sleeper draft between picks, or one that has not
 * started, has nothing to say and the cell stays out of the way. A Yahoo
 * draft never has a deadline to show at all, and silence there looked like a
 * timer that had not started yet.
 */
function whyNoClock(
  platform: Platform,
  status: string,
  complete: boolean,
  preDraft: boolean,
): string | null {
  if (platform !== "yahoo" || complete || preDraft || status !== "drafting") return null;
  return "no clock from Yahoo";
}

export function ClockBanner({ view }: { view: DraftView }) {
  const d = view.draft;
  const preDraft = d.status === "pre_draft" && d.total_picks_made === 0;
  const complete = d.status === "complete";
  // Nothing counts down while the draft is stopped. The backend already
  // withholds the deadline, and this makes the banner right even against an
  // older host that does not.
  const deadline = complete || d.paused ? null : d.clock_deadline_ms;
  const now = useNow(deadline !== null);
  const clock = clockLabel(deadline, now);
  // Everything that decides what the sentence says, and nothing that ticks.
  const situation = `${d.status}|${String(d.is_my_pick)}|${d.current_pick}|${d.on_clock_slot}`;
  const announcement = useHeldSentence(situation, clockSentence(d, secondsLeft(deadline, now)));
  // Four picks and a "+n", the same shape the queue strip uses — the list used
  // to stop at four and say nothing about the rest of the draft.
  const shownPicks = d.my_next_picks.slice(0, COLLAPSED);
  const morePicks = d.my_next_picks.length - shownPicks.length;
  const noClock = whyNoClock(view.league.platform, d.status, complete, preDraft);

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
      {/* The one thing in the app worth interrupting someone for. The visible
          wording is hidden from screen readers so the region reads as the one
          sentence above rather than saying half of it twice. */}
      <div className="clock-main" role="status" aria-live="assertive" aria-atomic="true">
        <span className="sr-only">{announcement}</span>
        {complete ? (
          <span className="clock-status" aria-hidden="true">
            Draft complete
          </span>
        ) : preDraft ? (
          <span className="clock-status" aria-hidden="true">
            Draft has not started
          </span>
        ) : d.paused ? (
          <span className="clock-status" aria-hidden="true">
            Draft paused
          </span>
        ) : d.is_my_pick ? (
          <span className="clock-status is-you" aria-hidden="true">
            You are on the clock
          </span>
        ) : (
          <>
            <span className="clock-status" aria-hidden="true">
              On the clock: {d.on_clock_name ?? `Slot ${d.on_clock_slot}`}
            </span>
            {d.picks_until_mine !== null && (
              <span className="mid clock-sub" aria-hidden="true">
                {d.picks_until_mine} pick{d.picks_until_mine === 1 ? "" : "s"} until you
              </span>
            )}
          </>
        )}
      </div>
      {clock !== null ? (
        <div className="clock-cell">
          <span className="label">Clock</span>
          <span className="clock-big num clock-timer">{clock}</span>
        </div>
      ) : (
        noClock !== null && (
          // Yahoo's draft feed carries no per-pick deadline, so the cell was
          // simply missing there — and a missing clock reads as a clock that
          // has not started rather than one that will never come.
          <div className="clock-cell">
            <span className="label">Clock</span>
            <span className="mid clock-sub">{noClock}</span>
          </div>
        )
      )}
      <div className="clock-cell clock-next">
        <span className="label">Your picks</span>
        <span className="clock-next-list num">
          {shownPicks.map((p) => pickLabel(p, d.teams)).join(" · ") || "–"}
          {morePicks > 0 && <span className="muted"> +{morePicks}</span>}
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

/**
 * The next two dozen picks, in the order they will actually happen.
 *
 * Two things the plain snake below cannot know about, so the backend hands
 * them over: `overrides` names the slot for any pick that changed hands or
 * was moved by third-round reversal, and `keepers` lists the picks that are
 * already in the book — a keeper is nobody's turn and never reaches a clock.
 */
function buildQueue(
  d: DraftView["draft"],
  rosters: TeamRoster[],
  keepers: Set<number>,
): QueueEntry[] {
  const { current_pick: currentPick, teams, rounds, my_slot: mySlot } = d;
  const total = teams * rounds;
  const entries: QueueEntry[] = [];
  const names = new Map(rosters.map((r) => [r.slot, r.display_name]));
  for (let pick = currentPick; pick <= total && entries.length < QUEUE_MAX; pick += 1) {
    if (keepers.has(pick)) continue;
    const slot = d.pick_slot_overrides[String(pick)] ?? slotForPick(pick, teams);
    entries.push({
      pickNo: pick,
      label: pickLabel(pick, teams),
      team: names.get(slot) ?? `Slot ${slot}`,
      isMine: mySlot === slot,
      onClock: pick === currentPick,
    });
  }
  return entries;
}

/**
 * Snake order: odd rounds run 1..n, even rounds run n..1.
 *
 * INVARIANT: this is the baseline `PickOwnership::overrides` (traded_picks.rs)
 * is measured against — it emits an entry for every pick a *plain* snake gets
 * wrong, and nothing else. So this function must stay a plain snake: teach it
 * about third-round reversal or traded picks and the two sides double-correct,
 * silently naming the wrong manager on exactly the picks that matter most.
 * Anything new the order depends on belongs in the overrides, not here.
 */
function slotForPick(pickNo: number, teams: number): number {
  const round = Math.floor((pickNo - 1) / teams) + 1;
  const index = (pickNo - 1) % teams;
  return round % 2 === 1 ? index + 1 : teams - index;
}

export function SnakeStrip({ view }: { view: DraftView }) {
  const [expanded, setExpanded] = useState(false);
  const draft = view.draft;
  const rosters = view.rosters;
  const clock = useClock(
    draft.status === "complete" || draft.paused ? null : draft.clock_deadline_ms,
  );
  // The queue is 24 picks of snake arithmetic and as many roster lookups, and
  // this strip re-renders every second while the clock runs.
  const queue = useMemo(
    () => buildQueue(draft, rosters, new Set(draft.keeper_picks)),
    [draft, rosters],
  );
  if (queue.length === 0) return null;

  const shown = expanded ? queue : queue.slice(0, COLLAPSED);
  // The last pick in the queue is drawn past the "+n" as a signpost — but only
  // when it is one of the hidden ones. With four or fewer picks left it is
  // already on screen, and drawing it again put the same chip up twice.
  const tail = expanded || queue.length <= COLLAPSED ? [] : queue.slice(-1);
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
              ? // The queue stops at QUEUE_MAX, so a full one is a floor
                // rather than the count of everything still to come.
                `${queue.length}${queue.length === QUEUE_MAX ? "+" : ""} picks ahead`
              : untilMine === 0
                ? "you are on the clock"
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
