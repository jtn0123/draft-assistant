// The player board: sortable on every column, filterable by position, with
// the loading and empty states the design specifies.

import { useEffect, useMemo, useRef, useState } from "react";
import type { AvailablePlayer, Position } from "../types";
import { SortHead } from "./bits";
import { BoardRow } from "./BoardRow";
import {
  COLUMNS,
  columnsFor,
  compare,
  SORT_LABEL,
  type Direction,
  type SortKey,
} from "./boardColumns";

const PAGE = 200;

/** Where the sort lands when nothing else has been chosen, and where it is put
 *  back when the column it was on stops existing. */
const FALLBACK_KEY = "pts" as const;
const FALLBACK_DIRECTION = "desc" as const;

const SKELETON_WIDTHS = ["72%", "88%", "64%", "80%", "70%", "84%"];

/** True when the key press landed somewhere already taking text. */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable;
}

/** Why the footer names a rank per position, in one hover. */
const REPLACEMENT_TITLE =
  "The last startable player at each position across the whole league, counting " +
  "the flex slots this league's pools actually earn. VORP is points over this player.";

export function Board({
  players,
  positions,
  loading,
  boardSize,
  replacementDemand,
  secondOpinionLoadedAt = null,
  onDraft,
}: {
  players: AvailablePlayer[];
  positions: Position[];
  loading: boolean;
  /** How many players the projections cover — named in the loading note. */
  boardSize: number;
  /** position -> league-wide startable count, flex share included. Drawn in
   *  the footer so the flex split VORP rests on is visible rather than
   *  implied. Omitted where the caller has none; the line then stays away. */
  replacementDemand?: Record<string, number>;
  /** When the imported projections CSV was read; named in the column tooltip.
   *  Omitted where the caller has no data health to hand — the column reads
   *  "imported" without a date rather than not appearing. */
  secondOpinionLoadedAt?: number | null;
  onDraft: (id: string, name: string) => void;
}) {
  const [pos, setPos] = useState<Position>("ALL");
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>(FALLBACK_KEY);
  const [direction, setDirection] = useState<Direction>(FALLBACK_DIRECTION);
  const [limit, setLimit] = useState(PAGE);
  const searchRef = useRef<HTMLInputElement>(null);

  // "/" jumps to search from anywhere on the screen, unless already typing.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) return;
      if (isTyping(event.target)) return;
      event.preventDefault();
      searchRef.current?.focus();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  const columns = useMemo(
    () => columnsFor(players, secondOpinionLoadedAt),
    [players, secondOpinionLoadedAt],
  );
  const showSecondOpinion = columns.length > COLUMNS.length;

  // "RB35 · WR37 · QB12 · TE12" — in league roster order, skipping any
  // position the model had no pool for.
  const replacementLine = useMemo(
    () =>
      positions
        .filter((p) => replacementDemand?.[p])
        .map((p) => `${p}${replacementDemand?.[p]}`)
        .join(" · "),
    [positions, replacementDemand],
  );

  // The imported column comes and goes with the projections behind it, and it
  // can be the one the board is sorted by. Falling back keeps the board sorted
  // by something — and keeps the footer's account of it true, rather than
  // naming a column that is no longer there while nothing is ordering the
  // rows.
  const sortable = columns.some((c) => c.key === sortKey);
  // The fallback has to be written back into the state, not merely used for
  // this render. While `sortKey` still named the vanished column, clicking the
  // header the board had fallen back to counted as "same column, flip it", and
  // the flip landed on a `direction` nothing was reading — so the header the
  // footer pointed at was the one header on the board that did nothing.
  // Adjusted here during render, which React re-runs immediately, rather than
  // in an effect that would paint the wrong sort first.
  if (!sortable) {
    setSortKey(FALLBACK_KEY);
    setDirection(FALLBACK_DIRECTION);
  }
  const activeKey: SortKey = sortable ? sortKey : FALLBACK_KEY;
  const activeDirection: Direction = sortable ? direction : FALLBACK_DIRECTION;

  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    const column = columns.find((c) => c.key === activeKey);
    const sign = activeDirection === "asc" ? 1 : -1;
    const filtered = players.filter(
      (p) => (pos === "ALL" || p.position === pos) && (!q || p.name.toLowerCase().includes(q)),
    );
    if (column === undefined) return filtered;
    // Decorate-sort-undecorate: the comparator runs O(n log n) times, and
    // `column.value` is doing string work on every one of those calls.
    return filtered
      .map((player) => ({ player, key: column.value(player) }))
      .sort((a, b) => compare(a.key, b.key, sign))
      .map(({ player }) => player);
    // `players` stays in the dependency list on purpose, and nothing coarser
    // belongs here: a rebuilt board can carry the same players with new
    // projections, so a key made of pick counts or lengths would render stale
    // numbers. The identity is instead made meaningful upstream — `applyView`
    // in App.tsx recycles this array whenever an incoming view says exactly
    // the same thing about the pool (boardIdentity.ts), so an update that only
    // moved the clock no longer re-filters and re-sorts the whole board.
  }, [players, columns, pos, query, activeKey, activeDirection]);

  // Paged rather than all-or-nothing. Every row carries two `PlayerName`s, a
  // headshot with its own state, effect and store subscription, and a logo —
  // so the old "Show all" put roughly 1,800 nodes and 600 store subscribers
  // into one synchronous commit. A page at a time keeps each commit the size
  // of the first one.
  const visible = matching.slice(0, limit);
  const hasFilters = pos !== "ALL" || query.trim() !== "";

  const sortBy = (key: SortKey) => {
    if (key === activeKey) {
      setDirection((d) => (d === "asc" ? "desc" : "asc"));
      return;
    }
    setSortKey(key);
    setDirection(columns.find((c) => c.key === key)?.initial ?? "asc");
  };

  // A filter change starts a different list, so it starts at the first page.
  // The page size was only ever put back by the "Show first 200" button:
  // page through to 800 rows, then search for one player, and the board still
  // committed 800 rows to show the one match — and the "Show first 200"
  // button that would have undone it was gone, because there was no longer
  // anything past the first page to hide.
  const choosePos = (next: Position) => {
    setPos(next);
    setLimit(PAGE);
  };

  const changeQuery = (next: string) => {
    setQuery(next);
    setLimit(PAGE);
  };

  const clearFilters = () => {
    choosePos("ALL");
    changeQuery("");
  };

  return (
    <div className="board">
      <div className="board-controls">
        <div className="board-tabs" role="group" aria-label="Filter players by position">
          {["ALL", ...positions].map((p) => (
            <button
              key={p}
              type="button"
              className={p === pos ? "board-tab is-on" : "board-tab"}
              onClick={() => choosePos(p)}
              aria-pressed={p === pos}
            >
              {p}
            </button>
          ))}
        </div>
        <input
          ref={searchRef}
          className="text-input board-search"
          placeholder="Search players — press /"
          aria-label="Search players"
          value={query}
          onChange={(e) => changeQuery(e.target.value)}
        />
        <span className="muted board-count" aria-live="polite">
          {matching.length > visible.length
            ? `Showing ${visible.length} of ${matching.length}`
            : `${matching.length} player${matching.length === 1 ? "" : "s"}`}
        </span>
      </div>

      <div className={`board-row board-head${showSecondOpinion ? " has-second" : ""}`}>
        {columns.map((column) => (
          <SortHead
            key={column.key}
            label={column.label}
            title={column.title}
            active={activeKey === column.key}
            direction={activeDirection}
            align={column.right ? "right" : undefined}
            onClick={() => sortBy(column.key)}
          />
        ))}
        <span />
      </div>

      {loading ? (
        // Announced, because a silent skeleton is indistinguishable from a
        // screen that has finished loading with nothing on it.
        <div className="board-loading" role="status">
          {SKELETON_WIDTHS.map((width, i) => (
            <div className="board-row board-skeleton" key={i}>
              <span className="skel" />
              <span className="skel" style={{ width }} />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
              <span className="skel" />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
              <span className="skel is-faint" />
            </div>
          ))}
          <div className="muted board-loading-note">
            {boardSize > 0
              ? `Pulling projections for ${boardSize} players…`
              : "Pulling projections…"}
          </div>
        </div>
      ) : matching.length === 0 ? (
        <div className="board-empty">
          <span className="board-empty-title">No players match</span>
          <span className="mid board-empty-note">
            {hasFilters
              ? "Nothing left at this position with the current filter. Clear it to see the full board."
              : "Every player on the board has been drafted."}
          </span>
          {hasFilters && (
            <button type="button" className="btn-ghost" onClick={clearFilters}>
              Clear filters
            </button>
          )}
        </div>
      ) : (
        visible.map((p) => (
          <BoardRow
            key={p.player_id}
            player={p}
            showSecondOpinion={showSecondOpinion}
            onDraft={onDraft}
          />
        ))
      )}

      <div className="board-foot">
        <span className="muted">
          Sorted by {SORT_LABEL[activeKey]},{" "}
          {activeDirection === "asc" ? "low to high" : "high to low"} · click any column
        </span>
        {replacementLine && (
          <span className="muted board-foot-replacement" title={REPLACEMENT_TITLE}>
            Replacement level: {replacementLine}
          </span>
        )}
        {matching.length > limit && (
          <button
            type="button"
            className="btn-ghost btn-row"
            onClick={() => setLimit((l) => l + PAGE)}
          >
            Show {Math.min(PAGE, matching.length - limit)} more
          </button>
        )}
        {limit > PAGE && (
          <button type="button" className="btn-ghost btn-row" onClick={() => setLimit(PAGE)}>
            Show first {PAGE}
          </button>
        )}
      </div>
    </div>
  );
}
