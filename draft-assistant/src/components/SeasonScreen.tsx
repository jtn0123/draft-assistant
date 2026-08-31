// The in-season screen: header stats, the main column (calls, lineup,
// waivers) and the tabbed rail.

import { useEffect, useRef, useState } from "react";
import type {
  SeasonHealth,
  SeasonTab,
  SeasonView,
  SourceHealth,
  SourceStatus,
} from "../season-types";
import { SEASON_TABS } from "../season-types";
import { age, fmt, lockLabel, nowSecs, pct, spanLabel, untilLabel } from "../format";
import { CallsToMake, LineupCompare, Waivers } from "./ThisWeek";
import { GamesTab } from "./GamesTab";
import { LastSeason, LeagueTab, Standings, TeamRoster } from "./SeasonTabs";
import { TrendsTab } from "./TrendsTab";

// Ship with this chunk, not with the window. board.css is here because the
// shared table header and the right/centre column helpers it owns are used by
// the standings and lineup tables as well as the draft board.
import "../board.css";
import "../season.css";
import "../season-tabs.css";
import "../trends.css";
import "../live.css";

/** Stable ids so each tab can point at the panel it controls. */
const tabId = (name: SeasonTab) => `rail-tab-${name.replace(/\s+/g, "-").toLowerCase()}`;
const panelId = (name: SeasonTab) => `rail-panel-${name.replace(/\s+/g, "-").toLowerCase()}`;

function HeaderStat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="season-stat">
      <span className="eyebrow">{label}</span>
      <span className="season-stat-value">{value}</span>
      {sub && <span className="muted small season-stat-sub">{sub}</span>}
    </div>
  );
}

/** How far behind a single source may fall before the badge stops vouching
 *  for it: three polls at the default thirty-second cadence. */
const SOURCE_STALE_SECS = 90;

/** How often the badge re-reads the clock, in milliseconds. */
const HEARTBEAT_MS = 10_000;

/**
 * The current time, refreshed on a timer.
 *
 * Everything else on this screen only changes when new data arrives, which is
 * exactly the wrong behaviour for a badge whose job is to notice that no new
 * data is arriving. Left to re-render on data alone it would sit there reading
 * "Live · 8s ago" for hours after the feed died.
 */
function useClockTick(): number {
  const [now, setNow] = useState(nowSecs);
  useEffect(() => {
    const id = setInterval(() => setNow(nowSecs()), HEARTBEAT_MS);
    return () => clearInterval(id);
  }, []);
  return now;
}

const SOURCES: [keyof SourceHealth, string][] = [
  ["matchups", "Matchups"],
  ["scores", "Scores"],
  ["rosters", "Rosters"],
];

/** "Scores: 5 seconds ago" / "Rosters: failing for 12 minutes (timeout)". */
function sourceLine(label: string, status: SourceStatus, now: number): string {
  const behind = Math.max(0, now - status.last_success_secs);
  if (status.error === null) return `${label}: ${spanLabel(behind)} ago`;
  const since =
    status.last_success_secs === 0 ? "never loaded" : `failing for ${spanLabel(behind)}`;
  return `${label}: ${since} (${status.error})`;
}

/**
 * The live badge, one source at a time.
 *
 * Overall freshness alone can be a lie: two feeds answering every thirty
 * seconds keep the stamp green while the third has been down for an hour. So
 * the badge counts how many sources are actually behind, and the tooltip
 * always spells out all three.
 */
function LiveBadge({ health }: { health: SeasonHealth }) {
  const now = useClockTick();
  const sources = health.sources;
  if (sources === undefined) {
    // Older cached views carry no per-source detail, only one overall stamp.
    // That stamp still has to be checked: an unchecked one always says "Live".
    const behind = now - health.fetched_at > SOURCE_STALE_SECS;
    return (
      <span className={behind ? "pill pill-stale" : "pill pill-live"}>
        <span className="dot" />
        {behind ? "Not updating" : `Live · ${age(health.fetched_at)}`}
      </span>
    );
  }
  const entries = SOURCES.map(([key, label]) => ({ label, status: sources[key] }));
  const behind = entries.filter(
    (e) => e.status.error !== null || now - e.status.last_success_secs > SOURCE_STALE_SECS,
  );
  const title = entries.map((e) => sourceLine(e.label, e.status, now)).join(" · ");

  if (behind.length === 0) {
    return (
      <span className="pill pill-live" title={title}>
        <span className="dot" />
        Live · {age(health.fetched_at)}
      </span>
    );
  }
  const names = behind.map((e) => e.label.toLowerCase()).join(" and ");
  return (
    <span className="pill pill-stale" title={title}>
      <span className="dot" />
      {behind.length === entries.length ? "Not updating" : `Live · ${names} behind`}
    </span>
  );
}

export function SeasonScreen({ view }: { view: SeasonView }) {
  const [tab, setTab] = useState<SeasonTab>("Standings");
  const selectedTab = useRef<HTMLButtonElement>(null);
  // Only move focus when the keyboard drove the change; clicking a tab should
  // not yank focus, and neither should the first render.
  const focusWanted = useRef(false);

  useEffect(() => {
    if (!focusWanted.current) return;
    focusWanted.current = false;
    selectedTab.current?.focus();
  }, [tab]);

  const onTabKey = (event: React.KeyboardEvent) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    let next: SeasonTab | null = null;
    if (step !== 0) {
      const at = SEASON_TABS.indexOf(tab);
      next = SEASON_TABS[(at + step + SEASON_TABS.length) % SEASON_TABS.length] ?? null;
    } else if (event.key === "Home") {
      next = SEASON_TABS[0] ?? null;
    } else if (event.key === "End") {
      next = SEASON_TABS[SEASON_TABS.length - 1] ?? null;
    }
    if (next === null) return;
    event.preventDefault();
    focusWanted.current = true;
    setTab(next);
  };
  const { header, matchup } = view;

  return (
    <div className="season-screen">
      <div className="season-header">
        <div className="season-stat is-lead">
          <span className="eyebrow">This week</span>
          <span className="season-stat-value ellipsis">
            {header.opponent_name === null
              ? `Week ${view.week} · bye`
              : `vs ${header.opponent_name} · ${fmt(header.my_projected, 1)} – ${fmt(header.opp_projected, 1)}`}
          </span>
        </div>
        <HeaderStat label="Win odds" value={pct(header.win_odds)} />
        <HeaderStat label="Playoffs" value={pct(header.playoff_odds)} />
        <HeaderStat
          label="Locks in"
          value={untilLabel(header.locks_in_ms)}
          sub={lockLabel(header.locks_in_ms)}
        />
        <div className="season-stat">
          <span className="eyebrow">Data</span>
          <LiveBadge health={view.data_health} />
        </div>
      </div>

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      <div className="season-body">
        <div className="season-main">
          <CallsToMake calls={view.calls} pointsOnTable={view.points_on_table} />
          <LineupCompare matchup={matchup} winOdds={header.win_odds} />
          <Waivers
            waivers={view.waivers}
            budgetLeft={view.waiver_budget_left}
            budgetTotal={view.waiver_budget_total}
            analysisAsOfSecs={view.analysis_as_of_secs}
          />
        </div>

        <div className="season-rail">
          <div className="rail-tabs" role="tablist" aria-label="League detail" onKeyDown={onTabKey}>
            {SEASON_TABS.map((name) => (
              <button
                key={name}
                type="button"
                role="tab"
                id={tabId(name)}
                aria-controls={panelId(name)}
                className={name === tab ? "rail-tab is-on" : "rail-tab"}
                aria-selected={name === tab}
                // Roving tabindex: Tab reaches the tablist, arrows move within
                // it — the interaction the tablist role already promises.
                tabIndex={name === tab ? 0 : -1}
                ref={name === tab ? selectedTab : undefined}
                onClick={() => setTab(name)}
              >
                {name}
              </button>
            ))}
          </div>

          <div role="tabpanel" id={panelId(tab)} aria-labelledby={tabId(tab)}>
            {tab === "Standings" && <Standings rows={view.standings} avatars={view.team_avatars} />}
            {tab === "Games" && (
              <GamesTab
                live={view.live}
                myProjected={header.my_projected}
                oppProjected={header.opp_projected}
                opponentName={header.opponent_name}
              />
            )}
            {tab === "My team" && <TeamRoster rows={view.roster} />}
            {tab === "Trends" && <TrendsTab trends={view.trends} avatars={view.team_avatars} />}
            {tab === "League" && (
              <LeagueTab
                trades={view.trades}
                recentTrades={view.recent_trades}
                activity={view.activity}
                avatars={view.team_avatars}
                analysisAsOfSecs={view.analysis_as_of_secs}
              />
            )}
            {tab === "Last season" && <LastSeason rows={view.last_season} season={view.season} />}
          </div>
        </div>
      </div>
    </div>
  );
}
