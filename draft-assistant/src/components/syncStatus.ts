import type { PollHealth } from "../types";

/** The beat the UI asks for while the draft is live. */
export const DRAFT_POLL_SECS = 3;

/**
 * Once the draft is over the picks feed never changes again, so the backend
 * slows its loop to `SEASON_IDLE` (`app_season.rs`) and re-reads the season
 * every half hour. Polling that slowly is right; calling it stale after the
 * draft's 30 s was not — the badge sat red for half of every minute all
 * season, on a feed that was working perfectly.
 *
 * So the threshold follows the beat: a feed is quiet when it has missed
 * several polls in a row, never less than 30 s.
 */
const SEASON_POLL_SECS = 60;
const MISSED_POLLS = 3;

export function quietSecs(seasonMode: boolean, intervalSecs = DRAFT_POLL_SECS): number {
  const beat = seasonMode ? Math.max(intervalSecs, SEASON_POLL_SECS) : intervalSecs;
  return Math.max(30, beat * MISSED_POLLS);
}

function ageSeconds(timestamp: number | null | undefined, now: number): number | null {
  if (!timestamp) return null;
  return Math.max(0, Math.floor(now / 1000 - timestamp));
}

export function syncClass(
  polling: boolean,
  health: PollHealth | null,
  now: number,
  seasonMode = false,
): string {
  if (!polling) return "";
  if ((health?.consecutive_failures ?? 0) >= 2) return "stale";
  if ((health?.consecutive_failures ?? 0) === 1) return "retrying";
  const quiet = (ageSeconds(health?.last_success_at, now) ?? 0) >= quietSecs(seasonMode);
  return quiet ? "stale" : "on";
}

export function syncLabel(
  polling: boolean,
  health: PollHealth | null,
  now: number,
  seasonMode = false,
): string {
  if (!polling) return "○ Live sync off";
  const failures = health?.consecutive_failures ?? 0;
  if (failures >= 2) return `● Sync stale · ${failures} failures`;
  if (failures === 1) return "● Sync retrying";
  const age = ageSeconds(health?.last_success_at, now);
  if (age !== null && age >= quietSecs(seasonMode)) {
    return `● Sync stale · nothing for ${formatAge(health?.last_success_at ?? 0, now)}`;
  }
  return "● Live sync on";
}

export function formatAge(timestamp: number, now: number): string {
  const seconds = ageSeconds(timestamp, now) ?? 0;
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m`;
}
