// The pure functions behind the Diagnostics dialog: how to describe the
// poller, the local clock, and what "Copy diagnostics" actually puts on the
// clipboard.
//
// In their own file because they are what the tests care about most — what
// leaves this machine when someone hits Copy — and because a component file
// that also exports helpers loses fast refresh.

import type { Diagnostics as Report } from "../types";

/** How the poller is doing, in the words the badge uses. */
export function pollSummary(report: Report): string {
  if (!report.polling) return "Off";
  const health = report.poll;
  if (health === null) return "On, nothing reported yet";
  if (health.consecutive_failures > 0) {
    return `Failing (${health.consecutive_failures} in a row): ${health.last_error ?? "no reason given"}`;
  }
  return health.last_success_at === null ? "On, no successful poll yet" : "On, healthy";
}

const two = (n: number): string => String(n).padStart(2, "0");

/** `-07:00` for a machine seven hours behind UTC, `+00:00` at UTC itself. */
export function utcOffset(date: Date): string {
  // getTimezoneOffset is minutes to ADD to local time to reach UTC, so it is
  // positive west of Greenwich; the sign on the wire is the other way round.
  const minutes = -date.getTimezoneOffset();
  const sign = minutes < 0 ? "-" : "+";
  const abs = Math.abs(minutes);
  return `${sign}${two(Math.floor(abs / 60))}:${two(abs % 60)}`;
}

/**
 * The machine's wall clock as `2026-09-05T20:01:02-07:00`.
 *
 * Every line of the log is stamped in UTC, which is right for sorting and
 * wrong for a person: "it broke around eight" has to be matched against
 * `03:02:11Z` by whoever reads the paste, and they get the day wrong as often
 * as not. One local timestamp with its offset at the top is the anchor that
 * makes the rest readable.
 */
export function localTimestamp(date: Date): string {
  const day = `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`;
  const time = `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
  return `${day}T${time}${utcOffset(date)}`;
}

/**
 * The block of text "Copy diagnostics" puts on the clipboard.
 *
 * Written out as plain lines rather than the JSON the backend hands over,
 * because the person on the other end of the paste is reading it, not parsing
 * it. Exported so a test can assert what leaves the machine. `now` is the
 * moment of the copy; only the tests pass it.
 */
export function diagnosticsText(
  report: Report,
  appVersion: string,
  now: Date = new Date(),
): string {
  const lines = [
    `Draft Assistant ${report.app_version === "" ? appVersion : report.app_version}`,
    `Copied at: ${localTimestamp(now)} (local time, UTC${utcOffset(now)}; log lines are UTC)`,
    `Platform: ${report.platform}`,
    `League: ${report.league_name ?? "none"} (${report.league_id ?? "-"}, ${report.platform_name ?? "-"})`,
    `Draft: ${report.draft_id ?? "none"}`,
    `Live sync: ${pollSummary(report)}`,
    `Phone & second screen: ${report.companion_enabled ? `on, ${report.companion_devices} paired` : "off"}`,
    `Log: ${report.log_path ?? "none on this machine"} (level ${report.log_level})`,
  ];
  if (report.log_tail.length > 0) lines.push("", "--- log ---", ...report.log_tail);
  return lines.join("\n");
}
