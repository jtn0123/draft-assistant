// The season screen's right rail: Standings, Games, My team, League, and
// Last season.

import { useMemo, useState } from "react";
import type {
  ActivityItem,
  LastSeasonRow,
  RosterRow,
  StandingsRow,
  TradeIdea,
  TradeDone,
} from "../season-types";
import { dateLabel, fmt, ideasAgeNote, ordinal, pct, signed } from "../format";
import { Headshot, PlayerName, PosBadge, SortHead, TeamAvatar, Empty } from "./bits";

// ---------- standings ----------

type StandingsKey = "seed" | "name" | "rec" | "proj" | "post";

const STANDINGS_COLUMNS: {
  key: StandingsKey;
  label: string;
  right: boolean;
  initial: "asc" | "desc";
  value: (r: StandingsRow) => string | number;
}[] = [
  { key: "seed", label: "#", right: true, initial: "asc", value: (r) => r.seed },
  { key: "name", label: "Team", right: false, initial: "asc", value: (r) => r.name },
  {
    key: "rec",
    label: "W–L",
    right: true,
    initial: "desc",
    value: (r) => r.wins * 1000 - r.losses,
  },
  { key: "proj", label: "Proj", right: true, initial: "desc", value: (r) => r.projected_points },
  { key: "post", label: "Post", right: true, initial: "desc", value: (r) => r.playoff_odds },
];

export function Standings({
  rows,
  avatars = {},
}: {
  rows: StandingsRow[];
  /** roster_id -> manager avatar; decoration, so absent is fine. */
  avatars?: Record<string, string>;
}) {
  const [key, setKey] = useState<StandingsKey>("seed");
  const [direction, setDirection] = useState<"asc" | "desc">("asc");

  const sorted = useMemo(() => {
    const column = STANDINGS_COLUMNS.find((c) => c.key === key);
    const sign = direction === "asc" ? 1 : -1;
    return rows.slice().sort((a, b) => {
      if (column === undefined) return 0;
      const x = column.value(a);
      const y = column.value(b);
      const cmp =
        typeof x === "string" && typeof y === "string" ? x.localeCompare(y) : Number(x) - Number(y);
      return cmp * sign;
    });
  }, [rows, key, direction]);

  const sortBy = (next: StandingsKey) => {
    if (next === key) {
      setDirection((d) => (d === "asc" ? "desc" : "asc"));
      return;
    }
    setKey(next);
    setDirection(STANDINGS_COLUMNS.find((c) => c.key === next)?.initial ?? "asc");
  };

  if (rows.length === 0) return <Empty>Standings load once the league has rosters.</Empty>;

  return (
    <div className="tab-body">
      <div className="standings-row standings-head">
        {STANDINGS_COLUMNS.map((column) => (
          <SortHead
            key={column.key}
            label={column.label}
            active={key === column.key}
            direction={direction}
            align={column.right ? "right" : undefined}
            onClick={() => sortBy(column.key)}
          />
        ))}
      </div>
      {sorted.map((row) => (
        <div
          className={row.is_mine ? "standings-row is-mine" : "standings-row"}
          key={row.roster_id}
        >
          <span className="muted right">{row.seed}</span>
          <span className="ellipsis team-cell">
            <TeamAvatar avatar={avatars[String(row.roster_id)]} name={row.name} />
            <span className="ellipsis">{row.name}</span>
          </span>
          <span className="right mid">{row.record}</span>
          <span className="right mid">{fmt(row.projected_points)}</span>
          <span className="right mid">{pct(row.playoff_odds)}</span>
        </div>
      ))}
      <span className="muted small tab-foot">
        Proj: best lineup each week from this roster, byes honoured. Post: simulated on the league
        schedule.
      </span>
    </div>
  );
}

// ---------- my team ----------

export function TeamRoster({ rows }: { rows: RosterRow[] }) {
  if (rows.length === 0) return <Empty>Set your Sleeper username to see your roster.</Empty>;
  const openSlots = rows.filter((r) => r.role === "Start").length;
  // Before the projections land, and before anyone has played a snap, both of
  // these are a column of "0.0" per player — which reads as fifteen men
  // measured at zero rather than as nothing measured yet. Em-dash the whole
  // column until one real number turns up in it.
  const anyProjected = rows.some((r) => r.projected > 0);
  const anyPoints = rows.some((r) => r.points !== 0);
  return (
    <div className="tab-body">
      <div className="tab-head">
        <span className="eyebrow">Roster</span>
        <span className="muted small">
          {rows.length} · {openSlots} starting
        </span>
      </div>
      <div className="team-row team-row-head">
        <span />
        <span className="muted small">Player</span>
        <span className="muted small right">Role</span>
        <span className="muted small right">Wk</span>
        <span className="muted small right">Season</span>
      </div>
      {rows.map((row) => (
        <div className={`team-row is-${row.role.toLowerCase()}`} key={row.player_id}>
          <PosBadge position={row.position} />
          <PlayerName name={row.name} team={row.team} playerId={row.player_id} />
          <span className="muted small right">{row.role}</span>
          <span className="right team-points">
            {row.role === "Bye" ? "Bye" : anyProjected ? fmt(row.projected, 1) : "—"}
          </span>
          <span className="right team-points team-season">
            {anyPoints ? fmt(row.points, 1) : "—"}
          </span>
        </div>
      ))}
      <span className="muted small tab-foot">
        Wk is this week's projection; Season is points to date. Bench points are dimmed; bye weeks
        show instead of a projection. A dash means that column has nothing in it yet.
      </span>
    </div>
  );
}

// ---------- league ----------

function RecentTrades({ deals, avatars }: { deals: TradeDone[]; avatars: Record<string, string> }) {
  const waiting = deals.filter((d) => d.pending).length;
  const note =
    deals.length === 0
      ? "none this week or last"
      : waiting === 0
        ? `${deals.length} completed`
        : `${waiting} in review · ${deals.length - waiting} completed`;
  return (
    <>
      <div className="tab-head">
        <span className="eyebrow">Trades in the league</span>
        <span className="muted small">{note}</span>
      </div>
      {deals.map((deal) => (
        <div
          className={deal.involves_me ? "trade-done is-mine" : "trade-done"}
          key={deal.transaction_id}
        >
          {deal.sides.map((side) => (
            <span className="trade-done-side" key={side.roster_id}>
              <TeamAvatar avatar={avatars[String(side.roster_id)]} name={side.team} />
              <strong>{side.team}</strong> gets{" "}
              {side.gets.length === 0 ? "draft picks" : side.gets.join(", ")}
            </span>
          ))}
          <span className="muted small">
            {deal.pending && <span className="tag tag-review">In review</span>}
            {dateLabel(deal.at / 1000, true)}
          </span>
        </div>
      ))}
    </>
  );
}

export function LeagueTab({
  trades,
  recentTrades,
  activity,
  avatars = {},
  analysisAsOfSecs,
}: {
  trades: TradeIdea[];
  recentTrades: TradeDone[];
  activity: ActivityItem[];
  avatars?: Record<string, string>;
  /** When the trade search last ran; absent or recent says nothing. */
  analysisAsOfSecs?: number;
}) {
  const ideasAge = ideasAgeNote(analysisAsOfSecs);
  return (
    <div className="tab-body">
      <RecentTrades deals={recentTrades} avatars={avatars} />
      <div className="tab-head tab-head-spaced">
        <span className="eyebrow">Trades worth offering</span>
        <span className="muted small">{ideasAge ?? "by roster fit"}</span>
      </div>
      {trades.length === 0 ? (
        <Empty>No swap would improve both rosters right now.</Empty>
      ) : (
        trades.map((trade) => (
          <div className="trade-row" key={`${trade.roster_id}-${trade.get_id}`}>
            <div className="trade-line">
              <span className="ellipsis trade-players">
                <PlayerName name={trade.get_name} team={trade.get_team} playerId={trade.get_id} />
                <span className="muted">for</span>
                <PlayerName
                  name={trade.give_name}
                  team={trade.give_team}
                  playerId={trade.give_id}
                />
              </span>
              <span className="trade-edge">{signed(trade.my_edge)} / wk</span>
            </div>
            <span className="muted small trade-partner">
              <TeamAvatar avatar={avatars[String(trade.roster_id)]} name={trade.partner} />
              {trade.note}
            </span>
          </div>
        ))
      )}

      <div className="tab-head tab-head-spaced">
        <span className="eyebrow">League activity</span>
        <span className="muted small">recent</span>
      </div>
      {activity.length === 0 ? (
        <Empty>Nothing has moved lately.</Empty>
      ) : (
        activity.map((item) => (
          <div className="activity-row" key={`${item.created}-${item.text}`}>
            <span className={`activity-kind is-${item.kind.toLowerCase()}`}>{item.kind}</span>
            <span className="activity-main">
              <span className="activity-text">
                {item.roster_id !== null && (
                  <TeamAvatar avatar={avatars[String(item.roster_id)]} name={item.text} />
                )}
                {item.text}
              </span>
              {item.players.length > 0 && (
                <span className="activity-faces">
                  {item.players.map((player) => (
                    <Headshot
                      key={player.id}
                      playerId={player.id}
                      team={player.team}
                      name={player.name}
                    />
                  ))}
                </span>
              )}
            </span>
            <span className="muted small activity-time">
              {dateLabel(item.created / 1000, true)}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

// ---------- last season ----------

export function LastSeason({ rows, season }: { rows: LastSeasonRow[]; season: string }) {
  if (rows.length === 0) {
    return <Empty>No previous season is linked to this league.</Empty>;
  }
  const mine = rows.find((r) => r.is_mine);
  const previous = Number(season) - 1;
  return (
    <div className="tab-body">
      <div className="tab-head">
        <span className="eyebrow">
          {Number.isNaN(previous) ? "Last season" : `${previous} final`}
        </span>
        {mine && <span className="muted small">you finished {ordinal(mine.place)}</span>}
      </div>
      {rows.map((row) => (
        <div
          className={row.is_mine ? "last-row is-mine" : "last-row"}
          key={`${row.place}-${row.name}`}
        >
          <span className="muted right">{row.place}</span>
          <span className="ellipsis">{row.name}</span>
          <span className="right mid">{row.record}</span>
          <span className="right mid">{fmt(row.points)}</span>
          <span className={tagClass(row.tag)}>{row.tag ?? ""}</span>
        </div>
      ))}
    </div>
  );
}

function tagClass(tag: string | null): string {
  if (tag === "Champ") return "right last-tag is-champ";
  if (tag === "Most pts") return "right last-tag is-most";
  return "right last-tag";
}
