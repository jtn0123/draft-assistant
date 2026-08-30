// Small display helpers shared across components.

export function fmt(n: number | null | undefined, digits = 0): string {
  if (n === null || n === undefined || Number.isNaN(n)) return "–";
  return n.toFixed(digits);
}

export function pct(p: number | null): string {
  if (p === null) return "–";
  return `${Math.round(p * 100)}%`;
}

/** Signed, one decimal, using a real minus sign: "+4.0", "−3.7". */
const DATE = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  timeZone: "America/New_York",
});
const DATE_TIME = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
  timeZone: "America/New_York",
});

/** "Sep 3" or "Sep 3, 11:30 PM", in Eastern time, from epoch seconds. */
export function dateLabel(secs: number, withTime = false): string {
  return (withTime ? DATE_TIME : DATE).format(new Date(secs * 1000));
}

export function signed(n: number, digits = 1): string {
  const sign = n > 0 ? "+" : "−";
  return `${sign}${Math.abs(n).toFixed(digits)}`;
}

/** Sleeper's team logo CDN. Returns null for players with no NFL team. */
export function teamLogo(team: string | null | undefined): string | null {
  if (!team) return null;
  return `https://sleepercdn.com/images/team_logos/nfl/${team.toLowerCase()}.png`;
}

/** "Thu Sep 10 · 8:15 PM ET" — the absolute time behind a countdown. */
export function lockLabel(ms: number | null | undefined): string {
  if (!ms) return "";
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: "America/New_York",
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).formatToParts(new Date(ms));
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${get("weekday")} ${get("month")} ${get("day")} · ${get("hour")}:${get("minute")} ${get("dayPeriod")} ET`;
}

/** "3.07" — the round.pick form used throughout the draft screen. */
export function pickLabel(pickNo: number, teams: number): string {
  if (teams <= 0) return String(pickNo);
  const round = Math.floor((pickNo - 1) / teams) + 1;
  const inRound = ((pickNo - 1) % teams) + 1;
  return `${round}.${String(inRound).padStart(2, "0")}`;
}

/** "4s ago", "3m ago". Ages are always shown relative, never as a clock. */
export function age(timestamp: number | null): string {
  if (timestamp === null) return "–";
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - timestamp));
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

/**
 * NFL kickoff windows are named in Eastern time regardless of where the user
 * is, so this formats explicitly in that zone: "Sun 1:00 ET".
 */
export function kickoffLabel(ms: number): string {
  if (!ms) return "";
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone: "America/New_York",
      weekday: "short",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    }).formatToParts(new Date(ms));
    const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
    return `${get("weekday")} ${get("hour")}:${get("minute")} ET`;
  } catch {
    return new Date(ms).toLocaleString();
  }
}

/** "2d 6h", "6h 12m", "48m" — how long until a deadline. */
export function untilLabel(ms: number | null): string {
  if (ms === null) return "–";
  const diff = ms - Date.now();
  if (diff <= 0) return "now";
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (days > 0) return `${days}d ${hours % 24}h`;
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  return `${minutes}m`;
}

/** Position with its rank appended, as the board shows it: "WR14". */
export function posRank(position: string, rank: number | null): string {
  return rank === null ? position : `${position}${rank}`;
}

/** "full-PPR" / "half-PPR" / "standard", from a league's reception value. */
export function scoringFormat(rec: number | null | undefined): string {
  if (rec === null || rec === undefined || Number.isNaN(rec) || rec <= 0) return "standard";
  if (rec >= 1) return "full-PPR";
  return rec >= 0.5 ? "half-PPR" : `${rec}-PPR`;
}

/** "0:41" — a pick clock, clamped at zero. Null when nothing is on the clock. */
export function clockLabel(deadlineMs: number | null, nowMs: number): string | null {
  if (deadlineMs === null) return null;
  const seconds = Math.max(0, Math.ceil((deadlineMs - nowMs) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
