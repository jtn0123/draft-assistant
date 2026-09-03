// The player board: sortable on every column, filterable by position, with
// the loading and empty states the design specifies.

import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { AvailablePlayer, Position } from "../types";
import { fmt, injuryTag, pct } from "../format";
import { PlayerName, SortHead } from "./bits";
import {
  hasSecondOpinion,
  secondOpinionRank,
  secondOpinionSource,
  secondOpinionTitle,
} from "../secondOpinion";
import { SecondOpinionCell } from "./SecondOpinion";

const PAGE = 200;
const SKELETON_WIDTHS = ["72%", "88%", "64%", "80%", "70%", "84%"];

type SortKey =
  "rank" | "name" | "pos" | "second" | "team" | "bye" | "pts" | "vorp" | "tier" | "adp" | "surv";

type Direction = "asc" | "desc";

interface Column {
  key: SortKey;
  label: string;
  right: boolean;
  title?: string;
  /** Natural first direction — points sort high-to-low, names A-to-Z. */
  initial: Direction;
  value: (p: AvailablePlayer) => string | number | null;
}

const COLUMNS: Column[] = [
  { key: "rank", label: "#", right: true, initial: "asc", value: (p) => p.overall_rank },
  { key: "name", label: "Player", right: false, initial: "asc", value: (p) => p.name },
  { key: "pos", label: "Pos", right: false, initial: "asc", value: (p) => p.position },
  { key: "team", label: "Team", right: false, initial: "asc", value: (p) => p.team },
  { key: "bye", label: "Bye", right: true, initial: "asc", value: (p) => p.bye_week },
  {
    key: "pts",
    label: "Pts",
    right: true,
    initial: "desc",
    title: "Season points under your league's exact scoring",
    value: (p) => p.points,
  },
  {
    key: "vorp",
    label: "Vorp",
    right: true,
    initial: "desc",
    title: "Value over replacement",
    value: (p) => p.vorp,
  },
  { key: "tier", label: "Tier", right: true, initial: "asc", value: (p) => p.tier },
  { key: "adp", label: "Adp", right: true, initial: "asc", value: (p) => p.adp },
  {
    key: "surv",
    label: "Surv",
    right: true,
    initial: "asc",
    title: "Chance they survive to your next pick",
    value: (p) => p.survival_next,
  },
];

/** The imported column, built only when there is something to put in it. It
 *  sits immediately after this board's own positional rank, which is the
 *  number it is there to be compared against. */
function columnsFor(players: AvailablePlayer[], loadedAt: number | null): Column[] {
  if (!hasSecondOpinion(players)) return COLUMNS;
  const source = secondOpinionSource(players);
  const at = COLUMNS.findIndex((c) => c.key === "pos") + 1;
  const column: Column = {
    key: "second",
    label: source,
    right: true,
    initial: "asc",
    title: secondOpinionTitle(source, loadedAt),
    value: secondOpinionRank,
  };
  return [...COLUMNS.slice(0, at), column, ...COLUMNS.slice(at)];
}

const SORT_LABEL: Record<SortKey, string> = {
  rank: "rank",
  name: "name",
  pos: "position",
  second: "the imported rank",
  team: "team",
  bye: "bye week",
  pts: "points",
  vorp: "VORP",
  tier: "tier",
  adp: "ADP",
  surv: "survival",
};

/** Order two cells, blanks last whichever way the column points.
 *
 * The direction is applied in here rather than by the caller: a blank that
 * answered "after you" would have become "before you" the moment the sort was
 * flipped, floating every free agent to the top of a descending Team sort.
 */
function compare(a: string | number | null, b: string | number | null, sign: number): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  if (typeof a === "string" && typeof b === "string") return a.localeCompare(b) * sign;
  return (Number(a) - Number(b)) * sign;
}

/** True when the key press landed somewhere already taking text. */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable;
}

/** One board row.
 *
 * Memoised because the whole board re-renders on every 3-second poll, and
 * each row carries two `PlayerName`s whose headshots subscribe to the avatar
 * store — 200 rows meant ~400 subscribers churning per tick for a list whose
 * contents usually did not change.
 */
const BoardRow = memo(function BoardRow({
  player: p,
  showSecondOpinion,
  onDraft,
}: {
  player: AvailablePlayer;
  showSecondOpinion: boolean;
  onDraft: (id: string, name: string) => void;
}) {
  return (
    <div className={`board-row board-body${showSecondOpinion ? " has-second" : ""}`}>
      <span className="muted right">{p.overall_rank}</span>
      <span className="board-player">
        <PlayerName
          name={p.name}
          team={p.team}
          tag={injuryTag(p.injury_status)}
          playerId={p.player_id}
        />
      </span>
      <span className={`pos-badge pos-${p.position}`}>
        <span>{p.position}</span>
        <span className="pos-rank">{p.position_rank}</span>
      </span>
      {showSecondOpinion && <SecondOpinionCell player={p} />}
      <span className="mid board-team">
        <PlayerName name={p.team ?? "–"} team={p.team} />
      </span>
      <span className="mid right">{p.bye_week ?? "–"}</span>
      <span className="strong right">{fmt(p.points)}</span>
      <span className="mid right">{fmt(p.vorp)}</span>
      <span className={`right tier tier-${Math.min(p.tier, 3)}`}>T{p.tier}</span>
      <span className="mid right">{fmt(p.adp)}</span>
      <span className={`right ${survClass(p.survival_next)}`}>{pct(p.survival_next)}</span>
      <span className="right">
        <button
          type="button"
          className="btn-ghost btn-row"
          onClick={() => onDraft(p.player_id, p.name)}
        >
          Draft
        </button>
      </span>
    </div>
  );
});

export function Board({
  players,
  positions,
  loading,
  boardSize,
  secondOpinionLoadedAt = null,
  onDraft,
}: {
  players: AvailablePlayer[];
  positions: Position[];
  loading: boolean;
  /** How many players the projections cover — named in the loading note. */
  boardSize: number;
  /** When the imported projections CSV was read; named in the column tooltip.
   *  Omitted where the caller has no data health to hand — the column reads
   *  "imported" without a date rather than not appearing. */
  secondOpinionLoadedAt?: number | null;
  onDraft: (id: string, name: string) => void;
}) {
  const [pos, setPos] = useState<Position>("ALL");
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("pts");
  const [direction, setDirection] = useState<Direction>("desc");
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

  // The imported column comes and goes with the projections behind it, and it
  // can be the one the board is sorted by. Falling back keeps the board sorted
  // by something — and keeps the footer's account of it true, rather than
  // naming a column that is no longer there while nothing is ordering the
  // rows. The chosen key is left alone, so the column coming back restores it.
  const sortable = columns.some((c) => c.key === sortKey);
  const activeKey: SortKey = sortable ? sortKey : "pts";
  const activeDirection: Direction = sortable ? direction : "desc";

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

  const clearFilters = () => {
    setPos("ALL");
    setQuery("");
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
              onClick={() => setPos(p)}
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
          onChange={(e) => setQuery(e.target.value)}
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

function survClass(p: number | null): string {
  if (p === null) return "muted";
  if (p <= 0.25) return "surv-low";
  if (p >= 0.75) return "surv-high";
  return "mid";
}
