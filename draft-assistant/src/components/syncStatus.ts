import type { PollHealth } from "../types";

// Polls run every 3s, so nothing at all for this long means the feed has
// stopped even when no single poll reported an error — the case a green pill
// used to hide.
const QUIET_SECS = 30;

function ageSeconds(timestamp: number | null | undefined, now: number): number | null {
  if (!timestamp) return null;
  return Math.max(0, Math.floor(now / 1000 - timestamp));
}

export function syncClass(polling: boolean, health: PollHealth | null, now: number): string {
  if (!polling) return "";
  if ((health?.consecutive_failures ?? 0) >= 2) return "stale";
  if ((health?.consecutive_failures ?? 0) === 1) return "retrying";
  return (ageSeconds(health?.last_success_at, now) ?? 0) >= QUIET_SECS ? "stale" : "on";
}

export function syncLabel(polling: boolean, health: PollHealth | null, now: number): string {
  if (!polling) return "○ Live sync off";
  const failures = health?.consecutive_failures ?? 0;
  if (failures >= 2) return `● Sync stale · ${failures} failures`;
  if (failures === 1) return "● Sync retrying";
  const age = ageSeconds(health?.last_success_at, now);
  if (age !== null && age >= QUIET_SECS) {
    return `● Sync stale · nothing for ${formatAge(health?.last_success_at ?? 0, now)}`;
  }
  return "● Live sync on";
}

export function formatAge(timestamp: number, now: number): string {
  const seconds = ageSeconds(timestamp, now) ?? 0;
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m`;
}
