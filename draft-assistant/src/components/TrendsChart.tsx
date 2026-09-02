// The Trends line chart: fourteen teams' projected strength over time, with a
// crosshair that reads off the nearest snapshot.
//
// Split from TrendsTab because this is the only part with a viewport, a
// coordinate space and a pointer to track; the tab around it is rows of text.
// The plot geometry is computed once per series and held across the several
// hover samples a frame brings.

import { memo, useMemo, useState } from "react";
import type { TeamSeries } from "../season-types";
import { dateLabel, fmt } from "../format";
import { extent, timeline } from "./trendsSeries";

const W = 460;
const H = 200;
const PAD = { top: 10, right: 12, bottom: 22, left: 34 };

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

export function TrendsChart({
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
