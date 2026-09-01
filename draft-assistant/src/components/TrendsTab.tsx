// The Trends tab: every team's projected strength over time, and a feed that
// says why each line moved (trades, claims, drops, injuries, projections).
//
// Fourteen teams is too many hues, so colour carries one identity only:
// mine. Every other team is a recessive line that lights up on hover or when
// picked in the legend — identity comes from the label, not the colour.

import { memo, useMemo, useState } from "react";
import type { TeamSeries, TrendChange, TrendsView } from "../season-types";
import { dateLabel, fmt, signed } from "../format";
import { Empty, PanelHead, Segmented, TeamAvatar } from "./bits";

const W = 460;
const H = 200;
const PAD = { top: 10, right: 12, bottom: 22, left: 34 };

/**
 * The smallest and largest of a list of numbers.
 *
 * `Math.min(...values)` reads better and is a latent crash: it spreads every
 * point of every series into one argument list, and a league with enough
 * snapshots behind it eventually crosses the engine's argument limit and
 * throws `RangeError: too many arguments`. A fold has no such ceiling.
 */
function extent(values: number[]): { lo: number; hi: number } {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return { lo, hi };
}

/** The distinct snapshot times across every series, oldest first. */
function timeline(series: TeamSeries[]): number[] {
  const seen = new Set<number>();
  for (const s of series) for (const p of s.points) seen.add(p.at);
  return [...seen].sort((a, b) => a - b);
}

/** True once at least one team's line actually goes somewhere. Two readings
 * of identical numbers plot as fourteen flat rules that read as gridlines. */
function hasMovement(series: TeamSeries[]): boolean {
  return series.some((s) => {
    if (s.points.length <= 1) return false;
    const { lo, hi } = extent(s.points.map((p) => p.strength));
    return hi - lo >= 0.05;
  });
}

/** Where everyone stands right now, on one shared scale, with a tail back to
 * where they started. A dot rather than a bar because the interesting range
 * starts well above zero, and this rather than fourteen near-flat lines
 * whenever there are too few readings for a line to say anything. */
function Ranked({
  series,
  avatars,
  showMovement,
}: {
  series: TeamSeries[];
  avatars: Record<string, string>;
  /** False before any team has actually moved: a column of "0.0" reads as
   *  measured stillness rather than as nothing having been measured. */
  showMovement: boolean;
}) {
  const now = series
    .map((s) => ({
      s,
      v: s.points[s.points.length - 1]?.strength ?? null,
      from: s.points[0]?.strength ?? null,
    }))
    .filter((r): r is { s: TeamSeries; v: number; from: number } => r.v !== null && r.from !== null)
    .sort((a, b) => b.v - a.v);
  if (now.length === 0) return null;
  const bounds = extent(now.flatMap((r) => [r.v, r.from]));
  const lo = Math.floor(bounds.lo / 5) * 5;
  const hi = Math.ceil(bounds.hi / 5) * 5;
  // Position along the shared track, 0..100. Named for what it is rather than
  // `pct`, which in this codebase is format.ts's "0.62 -> 62%".
  const along = (v: number) => ((v - lo) / Math.max(1, hi - lo)) * 100;
  return (
    <div className="trend-ranked">
      {now.map(({ s, v, from }) => {
        const delta = v - from;
        const flat = Math.abs(delta) < 0.05;
        return (
          <div
            className={s.is_mine ? "trend-rank-row is-mine" : "trend-rank-row"}
            key={s.roster_id}
          >
            <span className="ellipsis team-cell">
              <TeamAvatar avatar={avatars[String(s.roster_id)]} name={s.name} />
              <span className="ellipsis">{s.name}</span>
            </span>
            <span className="trend-track">
              {!flat && (
                <span
                  className="trend-tail"
                  style={{
                    left: `${Math.min(along(from), along(v))}%`,
                    width: `${Math.abs(along(v) - along(from))}%`,
                  }}
                />
              )}
              <span
                className="trend-mark"
                style={{ left: `${along(v)}%` }}
                title={`${fmt(v, 1)}/wk`}
              />
            </span>
            <span className="trend-right mid">{fmt(v, 1)}</span>
            <span
              className={
                !showMovement || flat
                  ? "trend-right muted"
                  : delta > 0
                    ? "trend-right trend-pos"
                    : "trend-right trend-act"
              }
            >
              {!showMovement ? "—" : flat ? "0.0" : signed(delta)}
            </span>
          </div>
        );
      })}
      <div className="trend-scale muted small" aria-hidden="true">
        <span>{fmt(lo, 0)}</span>
        <span>{fmt(hi, 0)} pts/wk</span>
      </div>
    </div>
  );
}

interface Plot {
  /** Distinct snapshot times, oldest first. */
  times: number[];
  /** Data space to SVG space. */
  x: (at: number) => number;
  y: (v: number) => number;
  /** The three horizontal gridlines, in data space. */
  ticks: number[];
  /** One path string per series, in the order the series were given. */
  paths: string[];
}

/**
 * Everything the data alone decides: the scales, the gridlines, and the path
 * string for each team.
 *
 * Hovering the chart samples a pointer position several times a frame. All
 * that hover changes is where a crosshair and two dots sit, so none of this —
 * fourteen path strings over every snapshot ever taken — has any business
 * being rebuilt for it.
 */
function buildPlot(series: TeamSeries[]): Plot {
  const times = timeline(series);
  const bounds = extent(series.flatMap((s) => s.points.map((p) => p.strength)));
  const lo = Math.floor(bounds.lo / 5) * 5;
  const hi = Math.ceil(bounds.hi / 5) * 5;
  const t0 = times[0];
  const t1 = times[times.length - 1];
  const x = (at: number) =>
    PAD.left + ((at - t0) / Math.max(1, t1 - t0)) * (W - PAD.left - PAD.right);
  const y = (v: number) =>
    PAD.top + (1 - (v - lo) / Math.max(1, hi - lo)) * (H - PAD.top - PAD.bottom);
  const paths = series.map((s) =>
    s.points.map((p, i) => `${i === 0 ? "M" : "L"}${x(p.at)},${y(p.strength)}`).join(" "),
  );
  return { times, x, y, ticks: [lo, (lo + hi) / 2, hi], paths };
}

/** The team lines, held still while the pointer moves over them. */
const Lines = memo(function Lines({
  series,
  paths,
  focus,
  onFocus,
}: {
  series: TeamSeries[];
  paths: string[];
  focus: number | null;
  onFocus: (rosterId: number | null) => void;
}) {
  return (
    <>
      {series.map((s, i) => (
        <path
          key={s.roster_id}
          className={
            s.is_mine
              ? "trend-line is-mine"
              : s.roster_id === focus
                ? "trend-line is-focus"
                : "trend-line"
          }
          d={paths[i]}
          onMouseEnter={() => onFocus(s.roster_id)}
          onMouseLeave={() => onFocus(null)}
        >
          <title>{s.name}</title>
        </path>
      ))}
    </>
  );
});

function Chart({
  series,
  focus,
  onFocus,
}: {
  series: TeamSeries[];
  focus: number | null;
  onFocus: (rosterId: number | null) => void;
}) {
  const [hoverAt, setHoverAt] = useState<number | null>(null);
  const { times, x, y, ticks, paths } = useMemo(() => buildPlot(series), [series]);
  const t0 = times[0];
  const t1 = times[times.length - 1];
  // Snapshots hours apart need the time to tell the ends of the axis apart.
  const sameDay = t1 - t0 < 86_400;
  const mine = series.find((s) => s.is_mine) ?? null;
  const focused = series.find((s) => s.roster_id === focus) ?? null;

  const nearest = (clientX: number, target: SVGSVGElement) => {
    const box = target.getBoundingClientRect();
    const px = ((clientX - box.left) / box.width) * W;
    let best = times[0];
    for (const t of times) if (Math.abs(x(t) - px) < Math.abs(x(best) - px)) best = t;
    setHoverAt(best);
  };

  const at = hoverAt ?? t1;
  const valueAt = (s: TeamSeries | null) => s?.points.find((p) => p.at === at)?.strength ?? null;

  return (
    <div className="trend-chart">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label="Projected strength per team over time"
        onMouseMove={(e) => nearest(e.clientX, e.currentTarget)}
        onMouseLeave={() => setHoverAt(null)}
      >
        {ticks.map((v) => (
          <g key={v}>
            <line className="trend-grid" x1={PAD.left} x2={W - PAD.right} y1={y(v)} y2={y(v)} />
            <text className="trend-tick" x={PAD.left - 6} y={y(v) + 3} textAnchor="end">
              {fmt(v, 0)}
            </text>
          </g>
        ))}
        <text className="trend-tick" x={PAD.left} y={H - 6}>
          {dateLabel(t0, sameDay)}
        </text>
        <text className="trend-tick" x={W - PAD.right} y={H - 6} textAnchor="end">
          {dateLabel(t1, sameDay)}
        </text>
        {hoverAt !== null && (
          <line
            className="trend-crosshair"
            x1={x(hoverAt)}
            x2={x(hoverAt)}
            y1={PAD.top}
            y2={H - PAD.bottom}
          />
        )}
        <Lines series={series} paths={paths} focus={focus} onFocus={onFocus} />
        {[mine, focused].map((s) => {
          const v = valueAt(s);
          if (s === null || v === null) return null;
          return (
            <circle
              key={s.roster_id}
              className={s.is_mine ? "trend-dot is-mine" : "trend-dot is-focus"}
              cx={x(at)}
              cy={y(v)}
              r={4}
            />
          );
        })}
      </svg>
      <div className="trend-tip small">
        <span className="mid">{dateLabel(at, true)}</span>
        {mine && valueAt(mine) !== null && (
          <span>
            <b>{mine.name}</b> {fmt(valueAt(mine) ?? 0, 1)}/wk
          </span>
        )}
        {focused && !focused.is_mine && valueAt(focused) !== null && (
          <span>
            {focused.name} {fmt(valueAt(focused) ?? 0, 1)}/wk
          </span>
        )}
      </div>
    </div>
  );
}

function Legend({
  series,
  focus,
  onFocus,
  avatars,
  showMovement,
}: {
  series: TeamSeries[];
  focus: number | null;
  onFocus: (rosterId: number | null) => void;
  avatars: Record<string, string>;
  /** Movement needs a run of readings behind it to mean anything — the same
   *  three the chart itself waits for. Below that the column is em-dashed
   *  rather than filled with the zeros of a series that has not moved yet. */
  showMovement: boolean;
}) {
  return (
    <div className="trend-legend" aria-label="Teams">
      {series.map((s) => {
        const last = s.points[s.points.length - 1];
        const first = s.points[0];
        const delta = last && first && s.points.length > 1 ? last.strength - first.strength : null;
        // Sub-tenth drift would print as "−0.0"; call it flat.
        const flat = delta !== null && Math.abs(delta) < 0.05;
        return (
          <button
            type="button"
            key={s.roster_id}
            className={
              s.is_mine
                ? "trend-legend-row is-mine"
                : s.roster_id === focus
                  ? "trend-legend-row is-focus"
                  : "trend-legend-row"
            }
            onMouseEnter={() => onFocus(s.roster_id)}
            onMouseLeave={() => onFocus(null)}
            onClick={() => onFocus(s.roster_id === focus ? null : s.roster_id)}
          >
            <span className="trend-swatch" aria-hidden="true" />
            <TeamAvatar avatar={avatars[String(s.roster_id)]} name={s.name} />
            <span className="ellipsis">{s.name}</span>
            <span className="trend-right mid">{last ? fmt(last.strength, 1) : "—"}</span>
            <span
              className={
                !showMovement || delta === null || flat
                  ? "trend-right muted"
                  : delta >= 0
                    ? "trend-right trend-pos"
                    : "trend-right trend-act"
              }
            >
              {!showMovement || delta === null ? "—" : flat ? "0.0" : signed(delta)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

function Feed({ changes }: { changes: TrendChange[] }) {
  if (changes.length === 0) {
    return <Empty>Nothing has moved yet. Trades, claims and injuries will show up here.</Empty>;
  }
  return (
    <div className="trend-feed">
      {changes.map((c, i) => (
        <div
          className={c.is_mine ? "trend-change is-mine" : "trend-change"}
          key={`${c.at}-${c.roster_id}-${i}`}
        >
          <div className="trend-change-head">
            <span className="ellipsis">
              <b>{c.team}</b>
            </span>
            <span className={c.delta >= 0 ? "trend-pos" : "trend-act"}>{signed(c.delta)} / wk</span>
          </div>
          <span className="mid small">
            {c.reasons.length > 0 ? c.reasons.join(" · ") : "projections moved"}
          </span>
          <span className="muted small">{dateLabel(c.at, true)}</span>
        </div>
      ))}
    </div>
  );
}

export function TrendsTab({
  trends,
  avatars = {},
}: {
  trends: TrendsView;
  /** roster_id -> manager avatar; decoration, so absent is fine. */
  avatars?: Record<string, string>;
}) {
  const [focus, setFocus] = useState<number | null>(null);
  const [mode, setMode] = useState<"Chart" | "Table">("Chart");
  const { series, changes } = trends;
  const snapshots = timeline(series).length;
  // Two readings plot as a slope; a dot plot with a tail says the same
  // thing far more legibly. Lines earn their place from the third on.
  const plottable = snapshots >= 3 && hasMovement(series);
  // The movement column answers "how far has this team come since the first
  // reading", so one reading cannot answer it: every row is its own baseline
  // and the whole column prints "0.0", which reads as fourteen teams measured
  // as standing still rather than as nothing measured yet. Em-dashed until a
  // second snapshot gives the subtraction something to subtract.
  const showMovement = snapshots >= 2;

  if (series.length === 0) {
    return <Empty>Trends start with the first Season load — check back after the next one.</Empty>;
  }

  return (
    <div className="tab-body">
      <PanelHead
        title="Projected strength"
        note={
          snapshots < 2
            ? "first snapshot taken — the graph fills in over time"
            : `${snapshots} snapshots`
        }
      />
      <div className="trend-controls">
        <span className="muted small">Best-lineup points per week, rest of season</span>
        <Segmented
          options={["Chart", "Table"] as const}
          value={mode}
          onChange={setMode}
          label="Trends view"
        />
      </div>
      {mode === "Chart" &&
        (plottable ? (
          <>
            <Chart series={series} focus={focus} onFocus={setFocus} />
            <Legend
              series={series}
              focus={focus}
              onFocus={setFocus}
              avatars={avatars}
              showMovement={showMovement}
            />
          </>
        ) : (
          <>
            <p className="empty-note">
              {snapshots < 2
                ? "One reading so far — here is where everyone stands. The line chart starts from the third."
                : `Where everyone stands, and how far they have moved across ${snapshots} readings. The line chart starts from the third.`}
            </p>
            <Ranked series={series} avatars={avatars} showMovement={showMovement} />
          </>
        ))}
      {mode === "Table" && (
        <Legend
          series={series}
          focus={focus}
          onFocus={setFocus}
          avatars={avatars}
          showMovement={showMovement}
        />
      )}
      <div className="tab-head-spaced">
        <PanelHead title="Why it moved" note="newest first" />
      </div>
      <Feed changes={changes} />
    </div>
  );
}
