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

/** Seconds since the epoch, now. Kept here with the other clock readers so
 * components stay pure functions of what they are handed. */
export function nowSecs(): number {
  return Math.floor(Date.now() / 1000);
}

/** "40 seconds", "1 minute", "3 hours" — a span of time in plain words. */
export function spanLabel(seconds: number): string {
  const plural = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"}`;
  const whole = Math.max(0, Math.floor(seconds));
  if (whole < 60) return plural(whole, "second");
  if (whole < 3600) return plural(Math.floor(whole / 60), "minute");
  return plural(Math.floor(whole / 3600), "hour");
}

/** How long the cached analysis has been sitting, but only once that is worth
 * saying: "ideas from 7 minutes ago". Null while the numbers are current. */
export function ideasAgeNote(asOfSecs: number | undefined, staleAfter = 120): string | null {
  if (asOfSecs === undefined || asOfSecs <= 0) return null;
  const seconds = Math.floor(Date.now() / 1000) - asOfSecs;
  if (seconds < staleAfter) return null;
  return `ideas from ${spanLabel(seconds)} ago`;
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

/**
 * The word behind a one-letter injury tag, for the tooltip beside a name.
 * Anything the backend does not recognise never reaches here.
 */
export function injuryWord(code: string): string {
  if (code === "Q") return "Questionable";
  if (code === "D") return "Doubtful";
  if (code === "O") return "Out";
  return code;
}

/**
 * Sleeper's raw `injury_status` reduced to the one-letter tag the rest of the
 * app draws. The season screen gets its codes from the backend
 * (`season_injury.rs`); the draft board carries the dictionary's own spelling,
 * which is a dozen words for three ideas — and "QUESTIONABLE" beside a name is
 * wider than the whole player column once the chat panel is open.
 *
 * Anything the list does not know ("Probable", a blank) is no tag at all.
 */
export function injuryTag(status: string | null | undefined): string | null {
  switch (status?.trim().toLowerCase()) {
    case "questionable":
    case "q":
      return "Q";
    case "doubtful":
    case "d":
      return "D";
    case "out":
    case "o":
    case "ir":
    case "pup":
    case "sus":
    case "susp":
    case "suspended":
    case "na":
    case "dnr":
    case "cov":
    case "covid":
      return "O";
    default:
      return null;
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

/** "1st", "2nd", "11th" — a place with its ordinal suffix. */
export function ordinal(n: number): string {
  const rest = n % 100;
  if (rest >= 11 && rest <= 13) return `${n}th`;
  return `${n}${["th", "st", "nd", "rd"][n % 10] ?? "th"}`;
}

/** What went wrong, in the app's own words, with the backend's kept on the
 * end rather than thrown away. */
export function problem(what: string, e: unknown): string {
  const detail = String(e)
    .replace(/^Error:\s*/, "")
    .trim();
  return detail === "" ? what : `${what} — ${detail}`;
}
