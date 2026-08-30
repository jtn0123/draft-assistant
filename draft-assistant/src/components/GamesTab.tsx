// Live NFL scoreboard, joined to the starters on both sides of the matchup.

import type { GameChip, LiveGame, LiveSection } from "../season-types";
import { fmt, kickoffLabel } from "../format";
import { TeamLogo, Empty } from "./bits";

function Chip({ chip }: { chip: GameChip }) {
  return (
    <span className={chip.is_mine ? "chip is-mine" : "chip"}>
      <span className={`chip-dot is-${chip.state}`} />
      {chip.slot} {chip.name}{" "}
      <span className={chip.is_mine && chip.state !== "pre" ? "chip-pts is-live" : "chip-pts"}>
        {fmt(chip.points, 1)}
      </span>
    </span>
  );
}

function GameRow({ game, compact }: { game: LiveGame; compact?: boolean }) {
  const status = game.status === "" ? kickoffLabel(game.kickoff_ms) : game.status;
  return (
    <div className={compact ? "game-row is-compact" : "game-row"}>
      <span className={`game-status is-${game.state}`}>{status}</span>
      <div className="game-detail">
        <div className="game-teams">
          <TeamLogo team={game.away} />
          <span className="strong">{game.away}</span>
          {game.away_score !== null && <span className="strong">{game.away_score}</span>}
          <span className="muted game-at">at</span>
          <TeamLogo team={game.home} />
          <span className="strong">{game.home}</span>
          {game.home_score !== null && <span className="strong">{game.home_score}</span>}
          {game.flag && <span className="game-flag">{game.flag}</span>}
          {game.channel !== null && game.state !== "final" && (
            <span className="game-channel">{game.channel}</span>
          )}
        </div>
        <div className="game-chips">
          {game.chips.map((chip) => (
            <Chip key={chip.player_id} chip={chip} />
          ))}
        </div>
      </div>
    </div>
  );
}

export function GamesTab({
  live,
  myProjected,
  oppProjected,
  opponentName,
}: {
  live: LiveSection;
  myProjected: number;
  oppProjected: number;
  opponentName: string | null;
}) {
  const { games, windows, totals, bye_teams: byeTeams } = live;
  if (games.length === 0) {
    return <Empty>No NFL games this week involve a starter on either roster.</Empty>;
  }

  const inProgress = games.filter((g) => g.state !== "pre");
  const myShare = share(totals.my_live_points, myProjected);
  const oppShare = share(totals.opp_live_points, oppProjected);
  const nextWindow = windows.find((w) => w.games.every((g) => g.state === "pre"));

  return (
    <div className="tab-body">
      <div className="tab-head">
        <span className="live-title">
          {inProgress.some((g) => g.state === "live") && <span className="live-dot" />}
          <span className="eyebrow">
            {inProgress.some((g) => g.state === "live") ? "Games in progress" : "This week"}
          </span>
        </span>
        <span className="muted small">
          {totals.my_playing} playing · {totals.my_done} done · {totals.my_pre} to play
        </span>
      </div>

      <div className="live-score">
        <div className="live-score-line">
          <span className="strong">You</span>
          <span className="live-score-num">{fmt(totals.my_live_points, 1)}</span>
        </div>
        <div className="live-track">
          <div className="live-fill is-mine" style={{ width: `${myShare}%` }} />
        </div>
        <div className="live-score-line">
          <span className="mid">{opponentName ?? "Opponent"}</span>
          <span className="live-score-num is-them">{fmt(totals.opp_live_points, 1)}</span>
        </div>
        <div className="live-track">
          <div className="live-fill is-theirs" style={{ width: `${oppShare}%` }} />
        </div>
        <span className="muted small">
          {myShare}% of your {fmt(myProjected, 1)} projection banked · they're at {oppShare}% of{" "}
          {fmt(oppProjected, 1)}
        </span>
      </div>

      {inProgress.length > 0 && (
        <div className="game-list">
          {inProgress.map((game) => (
            <GameRow key={game.game_id} game={game} />
          ))}
        </div>
      )}

      {nextWindow !== undefined && (
        <>
          <div className="tab-head tab-head-spaced">
            <span className="eyebrow">Next kickoff · {kickoffLabel(nextWindow.kickoff_ms)}</span>
            <span className="muted small">{nextWindow.my_starters} of your starters</span>
          </div>
          <div className="game-list">
            {nextWindow.games.map((game) => (
              <GameRow key={game.game_id} game={game} compact />
            ))}
          </div>
        </>
      )}

      <div className="live-counts">
        <div className="live-count">
          <span className="label">Playing</span>
          <span className="live-count-num is-live">{totals.my_playing}</span>
        </div>
        <div className="live-count">
          <span className="label">Yet to play</span>
          <span className="live-count-num">{totals.my_pre}</span>
        </div>
        <div className="live-count">
          <span className="label">Done</span>
          <span className="live-count-num is-done">{totals.my_done}</span>
        </div>
      </div>

      {windows.map((window) => (
        <div className="window" key={window.kickoff_ms}>
          <div className="window-head">
            <span className="eyebrow">{kickoffLabel(window.kickoff_ms)}</span>
            <span className="muted small">
              {window.my_starters} starter{window.my_starters === 1 ? "" : "s"}
            </span>
          </div>
          {window.games.map((game) => (
            <div className="window-game" key={game.game_id}>
              <div className="window-game-main">
                <div className="game-teams">
                  <TeamLogo team={game.away} />
                  <span className="strong">{game.away}</span>
                  <span className="muted game-at">at</span>
                  <TeamLogo team={game.home} />
                  <span className="strong">{game.home}</span>
                  {game.away_score !== null && (
                    <span className={game.state === "live" ? "strong" : "mid strong"}>
                      {game.away_score}–{game.home_score}
                    </span>
                  )}
                </div>
                <span className="muted small ellipsis">{rosterLine(game, opponentName)}</span>
              </div>
              <div className="window-game-when">
                <span className={`game-status is-${game.state}`}>
                  {game.status === "" ? kickoffLabel(game.kickoff_ms) : game.status}
                </span>
                {game.channel !== null && game.state !== "final" && (
                  <span className="game-channel">{game.channel}</span>
                )}
              </div>
            </div>
          ))}
        </div>
      ))}

      <span className="muted small tab-foot">
        {byeTeams.length > 0 && `Byes this week: ${byeTeams.join(", ")}. `}
        Live scoring updates every 30s while a game is in progress.
      </span>
    </div>
  );
}

function rosterLine(game: LiveGame, opponentName: string | null): string {
  const mine = game.chips.filter((c) => c.is_mine).map((c) => `${c.slot} ${c.name}`);
  const theirs = game.chips.filter((c) => !c.is_mine).map((c) => `${c.slot} ${c.name}`);
  const parts: string[] = [];
  if (mine.length) parts.push(`You: ${mine.join(", ")}`);
  if (theirs.length) parts.push(`${opponentName ?? "Them"}: ${theirs.join(", ")}`);
  return parts.length ? parts.join("  ·  ") : "no starters either side";
}

/** Percentage of a projection banked, clamped so a blowout can't overflow. */
function share(points: number, projected: number): number {
  if (projected <= 0) return 0;
  return Math.min(100, Math.round((points / projected) * 100));
}
