// The header's second line, from whichever screen is showing.
//
// Pulled out of App.tsx, which was closing on the 500-line cap; this is a
// pure function of the two views and reads better beside its own test than
// inline in the shell.

import { ordinal, scoringFormat } from "./format";
import type { Screen } from "./prefs";
import type { SeasonView } from "./season-types";
import type { DraftView } from "./types";

/** "Round 3 of 15 · 41 picks in" on the draft board; "Week 3 · 2-0 · 1st of
 *  12" on the season screen, or the season's year before it has loaded. */
export function headerSubtitle(screen: Screen, view: DraftView, season: SeasonView | null): string {
  if (screen === "draft") {
    const d = view.draft;
    return `Round ${d.current_round} of ${d.rounds} · ${d.total_picks_made} picks in`;
  }
  if (season === null) return `${view.league.season} season`;
  return `Week ${season.week} · ${myRecord(season)}`;
}

function myRecord(season: SeasonView): string {
  const mine = season.standings.find((s) => s.is_mine);
  if (mine === undefined) return `${season.standings.length} teams`;
  return `${mine.record} · ${ordinal(mine.seed)} of ${season.standings.length}`;
}

/** The header's third line: "12-team half-PPR · 15 rounds", plus a note while
 *  picks are being typed in by hand. */
export function headerMeta(view: DraftView): string {
  const d = view.draft;
  const manual = d.manual_picks_active ? " · manual picks active" : "";
  return `${d.teams}-team ${scoringFormat(view.league.scoring_settings.rec)} · ${d.rounds} rounds${manual}`;
}

/** The line every screen carries under the settings menu. Yahoo's terms ask
 *  for the attribution wherever their data is shown. */
export function footerNote(view: DraftView): string {
  if (view.league.platform === "yahoo") {
    return "Fantasy data provided by Yahoo Fantasy · read-only connection";
  }
  return `${view.league.name} · league ${view.league.league_id} · read-only connection`;
}
