// The player board: sortable on every column, filterable by position, with
// the loading and empty states the design specifies.

import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { AvailablePlayer, Position } from "../types";
import { fmt, pct } from "../format";
import { PlayerName, SortHead } from "./bits";

const PAGE = 200;
const SKELETON_WIDTHS = ["72%", "88%", "64%", "80%", "70%", "84%"];

type SortKey =
  | "rank"
  | "name"
  | "pos"
  | "team"
  | "bye"
  | "pts"
  | "vorp"
  | "tier"
  | "adp"
  | "surv";

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

const SORT_LABEL: Record<SortKey, string> = {
  rank: "rank",
  name: "name",
  pos: "position",
  team: "team",
  bye: "bye week",
  pts: "points",
  vorp: "VORP",
  tier: "tier",
  adp: "ADP",
  surv: "survival",
};

function compare(a: string | number | null, b: string | number | null): number {
  // Missing values sort last in either direction rather than reading as zero.
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  if (typeof a === "string" && typeof b === "string") return a.localeCompare(b);
  return Number(a) - Number(b);
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
  onDraft,
}: {
  player: AvailablePlayer;
  onDraft: (id: string, name: string) => void;
}) {
  return (
    <div className="board-row board-body">
      <span className="muted right">{p.overall_rank}</span>
      <span className="board-player">
        <PlayerName name={p.name} team={p.team} tag={p.injury_status} playerId={p.player_id} />
      </span>
      <span className={`pos-badge pos-${p.position}`}>
        <span>{p.position}</span>
        <span className="pos-rank">{p.position_rank}</span>
      </span>
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
  onDraft,
}: {
  players: AvailablePlayer[];
  positions: Position[];
  loading: boolean;
  /** How many players the projections cover — named in the loading note. */
  boardSize: number;
  onDraft: (id: string, name: string) => void;
}) {
  const [pos, setPos] = useState<Position>("ALL");
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("pts");
  const [direction, setDirection] = useState<Direction>("desc");
  const [showAll, setShowAll] = useState(false);
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

  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    const column = COLUMNS.find((c) => c.key === sortKey);
    const sign = direction === "asc" ? 1 : -1;
    return players
      .filter((p) => pos === "ALL" || p.position === pos)
      .filter((p) => !q || p.name.toLowerCase().includes(q))
      .slice()
      .sort((a, b) =>
        column === undefined ? 0 : compare(column.value(a), column.value(b)) * sign,
      );
  }, [players, pos, query, sortKey, direction]);

  const visible = showAll ? matching : matching.slice(0, PAGE);
  const hasFilters = pos !== "ALL" || query.trim() !== "";

  const sortBy = (key: SortKey) => {
    if (key === sortKey) {
      setDirection((d) => (d === "asc" ? "desc" : "asc"));
      return;
    }
    setSortKey(key);
    setDirection(COLUMNS.find((c) => c.key === key)?.initial ?? "asc");
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

      <div className="board-row board-head">
        {COLUMNS.map((column) => (
          <SortHead
            key={column.key}
            label={column.label}
            title={column.title}
            active={sortKey === column.key}
            direction={direction}
            align={column.right ? "right" : undefined}
            onClick={() => sortBy(column.key)}
          />
        ))}
        <span />
      </div>

      {loading ? (
        <div className="board-loading">
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
            {boardSize > 0 ? `Pulling projections for ${boardSize} players…` : "Pulling projections…"}
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
        visible.map((p) => <BoardRow key={p.player_id} player={p} onDraft={onDraft} />)
      )}

      <div className="board-foot">
        <span className="muted">
          Sorted by {SORT_LABEL[sortKey]}, {direction === "asc" ? "low to high" : "high to low"} ·
          click any column
        </span>
        {matching.length > PAGE && (
          <button
            type="button"
            className="btn-ghost btn-row"
            onClick={() => setShowAll((s) => !s)}
          >
            {showAll ? `Show first ${PAGE}` : `Show all ${matching.length}`}
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
