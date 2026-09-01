// The list the league picker shows, put together from two sources.
//
// One is the config: every league this app has already loaded, which is what
// makes switching to a mock draft and back a two-click job. The other is the
// Sleeper account itself, which knows about leagues this app has never seen.
// Neither list is authoritative on its own, and they overlap, so the merge
// lives here — pure, and tested on its own rather than through the dialog.

import type { StoredLeague } from "./types";

/**
 * Every league worth offering, once each, in the order they should be read.
 *
 * The active league leads: it is the one the user is checking against when
 * they open the picker. The rest follow by season, newest first, and then by
 * name, so a list that spans two seasons does not interleave them.
 *
 * @param stored what the app has loaded before, from the config
 * @param found what Sleeper says the account plays in, if it has been asked
 * @param activeId the league on screen right now
 */
export function mergeLeagues(
  stored: StoredLeague[],
  found: StoredLeague[],
  activeId: string | null,
): StoredLeague[] {
  const byId = new Map<string, StoredLeague>();
  for (const league of stored) byId.set(league.league_id, league);
  // Sleeper's answer wins on name and season: a league renamed there should
  // not keep showing whatever it was called when this app last loaded it.
  for (const league of found) byId.set(league.league_id, league);
  return [...byId.values()].sort((a, b) => {
    if (a.league_id === activeId) return -1;
    if (b.league_id === activeId) return 1;
    if (a.season !== b.season) return b.season.localeCompare(a.season);
    return a.name.localeCompare(b.name);
  });
}

/** What a league row says under its name. */
export function leagueNote(league: StoredLeague, activeId: string | null): string {
  const season = league.season === "" ? "season unknown" : `${league.season} season`;
  return league.league_id === activeId ? `${season} · on screen now` : season;
}
