import type { DraftView } from "./types";
import type { ViewMode } from "./viewMode";

/**
 * The line under the league name. On the draft screen, the draft's shape;
 * on the season screen, where the season is: the week, the record, who is
 * up next. Falls back to the draft line until there is a week to speak of.
 */
export function subtitle(view: DraftView, mode: ViewMode): string {
  const draft = [
    `${view.league.season} · ${view.draft.teams} teams · ${view.draft.rounds} rounds`,
    view.draft.manual_picks_active && "manual picks active",
  ]
    .filter(Boolean)
    .join(" · ");
  const w = view.this_week;
  if (mode !== "season" || !w) return draft;
  const mine = view.draft.my_slot;
  const record = view.season?.standings.find((r) => r.slot === mine) ?? null;
  const m = w.matchup;
  return [
    String(view.league.season),
    `Week ${w.week}`,
    record && `${record.wins}–${record.losses}${record.ties > 0 ? `–${record.ties}` : ""}`,
    m && `vs ${m.opponent_name ?? `slot ${m.opponent_slot}`}`,
  ]
    .filter(Boolean)
    .join(" · ");
}
