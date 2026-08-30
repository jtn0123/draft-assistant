// The in-season screen: header stats, the main column (calls, lineup,
// waivers) and the tabbed rail.

import { useState } from "react";
import type { SeasonTab, SeasonView } from "../season-types";
import { SEASON_TABS } from "../season-types";
import { fmt, lockLabel, pct, untilLabel } from "../format";
import { CallsToMake, LineupCompare, Waivers } from "./ThisWeek";
import { GamesTab } from "./GamesTab";
import { LastSeason, LeagueTab, Standings, TeamRoster } from "./SeasonTabs";
import { TrendsTab } from "./TrendsTab";

function HeaderStat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="season-stat">
      <span className="eyebrow">{label}</span>
      <span className="season-stat-value">{value}</span>
      {sub && <span className="muted small season-stat-sub">{sub}</span>}
    </div>
  );
}

export function SeasonScreen({ view }: { view: SeasonView }) {
  const [tab, setTab] = useState<SeasonTab>("Standings");
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
          />
        </div>

        <div className="season-rail">
          <div className="rail-tabs" role="tablist" aria-label="League detail">
            {SEASON_TABS.map((name) => (
              <button
                key={name}
                type="button"
                role="tab"
                className={name === tab ? "rail-tab is-on" : "rail-tab"}
                aria-selected={name === tab}
                onClick={() => setTab(name)}
              >
                {name}
              </button>
            ))}
          </div>

          {tab === "Standings" && (
            <Standings rows={view.standings} avatars={view.team_avatars} />
          )}
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
            />
          )}
          {tab === "Last season" && (
            <LastSeason rows={view.last_season} season={view.season} />
          )}
        </div>
      </div>
    </div>
  );
}
