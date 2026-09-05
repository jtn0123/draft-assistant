// The two pure functions behind the Diagnostics dialog: how to describe the
// poller, and what "Copy diagnostics" actually puts on the clipboard.
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

/**
 * The block of text "Copy diagnostics" puts on the clipboard.
 *
 * Written out as plain lines rather than the JSON the backend hands over,
 * because the person on the other end of the paste is reading it, not parsing
 * it. Exported so a test can assert what leaves the machine.
 */
export function diagnosticsText(report: Report, appVersion: string): string {
  const lines = [
    `Draft Assistant ${report.app_version === "" ? appVersion : report.app_version}`,
    `Platform: ${report.platform}`,
    `League: ${report.league_name ?? "none"} (${report.league_id ?? "-"}, ${report.platform_name ?? "-"})`,
    `Draft: ${report.draft_id ?? "none"}`,
    `Live sync: ${pollSummary(report)}`,
    `Phone & second screen: ${report.companion_enabled ? `on, ${report.companion_devices} paired` : "off"}`,
    `Log: ${report.log_path ?? "none on this machine"}`,
  ];
  if (report.log_tail.length > 0) lines.push("", "--- log ---", ...report.log_tail);
  return lines.join("\n");
}
