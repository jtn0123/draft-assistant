// The in-season screen: header stats, the main column (calls, lineup,
// waivers) and the tabbed rail.

import { useEffect, useRef, useState } from "react";
import type {
  LineupChoice,
  SeasonHealth,
  SeasonTab,
  SeasonView,
  SourceHealth,
  SourceStatus,
} from "../season-types";
import { SEASON_TABS } from "../season-types";
import type { PollHealth } from "../types";
import { age, fmt, lockLabel, nowSecs, pct, spanLabel, untilLabel } from "../format";
import { CallsToMake, LineupCompare, Waivers } from "./ThisWeek";
import { GamesTab } from "./GamesTab";
import { ODDS_NOTE } from "../odds";
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

interface BadgeStatus {
  label: string;
  /** True when the badge should stop vouching for the data it is stamping. */
  stale: boolean;
  /** One line per source, when the view carries the breakdown. */
  lines: string[];
  /** The same breakdown as one hover string, for a mouse. */
  title: string | undefined;
}

/**
 * What the badge says about freshness, one source at a time.
 *
 * Overall freshness alone can be a lie: two feeds answering every thirty
 * seconds keep the stamp green while the third has been down for an hour. So
 * the badge counts how many sources are actually behind, and the breakdown
 * spells out all three.
 */
function badgeStatus(health: SeasonHealth, now: number): BadgeStatus {
  const sources = health.sources;
  if (sources === undefined) {
    // Older cached views carry no per-source detail, only one overall stamp.
    // That stamp still has to be checked: an unchecked one always says "Live".
    const stale = now - health.fetched_at > SOURCE_STALE_SECS;
    return {
      label: stale ? "Not updating" : `Live · ${age(health.fetched_at)}`,
      stale,
      lines: [],
      title: undefined,
    };
  }
  const entries = SOURCES.map(([key, label]) => ({ label, status: sources[key] }));
  const behind = entries.filter(
    (e) => e.status.error !== null || now - e.status.last_success_secs > SOURCE_STALE_SECS,
  );
  const lines = entries.map((e) => sourceLine(e.label, e.status, now));
  const title = lines.join(" · ");
  if (behind.length === 0) {
    return { label: `Live · ${age(health.fetched_at)}`, stale: false, lines, title };
  }
  const names = behind.map((e) => e.label.toLowerCase()).join(" and ");
  return {
    label: behind.length === entries.length ? "Not updating" : `Live · ${names} behind`,
    stale: true,
    lines,
    title,
  };
}

/** Why the score feed is not updating, in the words a person would use. */
function failureNote(poll: PollHealth, now: number): string {
  const tries =
    poll.consecutive_failures === 1
      ? "The last try"
      : `The last ${poll.consecutive_failures} tries`;
  const since =
    poll.last_success_at === null
      ? "no scores have come through yet"
      : `the last new scores arrived ${spanLabel(Math.max(0, now - poll.last_success_at))} ago`;
  const why = poll.last_error === null ? "" : ` (${poll.last_error})`;
  return `${tries} to get new scores failed — ${since}${why}`;
}

/**
 * The badge, plus a sentence when the poller says it is failing.
 *
 * Staleness and failure are one status, not two competing ones. A failed poll
 * is the surer of the two — the timestamps can still look fresh for a minute
 * after the feed stops answering — so it decides the badge, and the reason
 * goes underneath where it can be read without hovering.
 */
function LiveStatus({ health, poll }: { health: SeasonHealth; poll: PollHealth | null }) {
  const now = useClockTick();
  const failing = poll !== null && poll.consecutive_failures > 0;
  const status = badgeStatus(health, now);
  return (
    <>
      <span
        className={failing || status.stale ? "pill pill-stale" : "pill pill-live"}
        title={status.title}
      >
        <span className="dot" />
        {failing ? "Not updating" : status.label}
      </span>
      {poll !== null && failing && (
        <span className="muted small season-stat-sub">{failureNote(poll, now)}</span>
      )}
      {/* Once something is behind, which feed and for how long is the whole
          question. It used to be a tooltip on a span with no way in from the
          keyboard; now it is written out under the badge. */}
      {(failing || status.stale) &&
        status.lines.map((line) => (
          <span key={line} className="muted small season-stat-sub">
            {line}
          </span>
        ))}
    </>
  );
}

export function SeasonScreen({
  view,
  pollHealth = null,
}: {
  view: SeasonView;
  /** The season poller's last report, or null before one has arrived. */
  pollHealth?: PollHealth | null;
}) {
  const [tab, setTab] = useState<SeasonTab>("Standings");
  // Which lineup the whole screen is talking about. It lived inside
  // LineupCompare while the header quoted best-lineup odds regardless, so the
  // screen could say "74% to win" and "2.8 sitting on your bench" at once.
  const [lineup, setLineup] = useState<LineupChoice>("Best");
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
  // One lineup, one score, one probability. Both numbers are computed in the
  // Rust view against the same opponent, so picking here cannot make them
  // disagree the way a single scalar and a local toggle could.
  const best = lineup === "Best";
  const myProjected = best ? header.my_projected : header.my_set_projected;
  const winOdds = best ? header.win_odds_best : header.win_odds_set;

  return (
    <div className="season-screen">
      <div className="season-header">
        <div className="season-stat is-lead">
          <span className="eyebrow">This week</span>
          <span className="season-stat-value ellipsis">
            {header.opponent_name === null
              ? `Week ${view.week} · bye`
              : `vs ${header.opponent_name} · ${fmt(myProjected, 1)} – ${fmt(header.opp_projected, 1)}`}
          </span>
        </div>
        {/* The note sits on the first of the two odds and speaks for both:
            one line, so a percentage is read as a model rather than a promise.
            Which lineup it prices comes first, because that is the half a
            reader can act on. */}
        <HeaderStat
          label="Win odds"
          value={pct(winOdds)}
          sub={`${best ? "best lineup" : "lineup as set"} · ${ODDS_NOTE}`}
        />
        <HeaderStat label="Playoffs" value={pct(header.playoff_odds)} />
        <HeaderStat
          label="Locks in"
          value={untilLabel(header.locks_in_ms)}
          sub={lockLabel(header.locks_in_ms)}
        />
        <div className="season-stat">
          <span className="eyebrow">Data</span>
          <LiveStatus health={view.data_health} poll={pollHealth} />
        </div>
      </div>

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      <div className="season-body">
        <div className="season-main">
          <CallsToMake calls={view.calls} pointsOnTable={view.points_on_table} />
          <LineupCompare matchup={matchup} which={lineup} onWhich={setLineup} winOdds={winOdds} />
          <Waivers
            waivers={view.waivers}
            budgetLeft={view.waiver_budget_left}
            budgetTotal={view.waiver_budget_total}
            analysisAsOfSecs={view.analysis_as_of_secs}
          />
        </div>

        <div className="season-rail">
          <div className="rail-tabs" role="tablist" aria-label="League detail">
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
                // The keys act on whichever tab has focus, so the handler
                // lives on the tabs rather than on the list around them.
                onKeyDown={onTabKey}
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
                myProjected={myProjected}
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
