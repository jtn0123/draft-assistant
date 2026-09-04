// The list the league picker shows, put together from two sources.
//
// One is the config: every league this app has already loaded, which is what
// makes switching to a mock draft and back a two-click job. The others are the
// accounts themselves — Sleeper's, and Yahoo's once it is connected — which
// know about leagues this app has never seen. No list is authoritative on its
// own, and they overlap, so the merge lives here — pure, and tested on its
// own rather than through the dialog.

import type { Platform, StoredLeague } from "./types";

/**
 * Every league worth offering, once each, in the order they should be read.
 *
 * The active league leads: it is the one the user is checking against when
 * they open the picker. The rest follow by season, newest first, and then by
 * name, so a list that spans two seasons does not interleave them.
 *
 * Leagues are keyed by id, which is enough to tell the platforms apart on
 * its own: Sleeper ids are digits and Yahoo keys look like `449.l.12345`.
 * A found league is still only allowed to rewrite a stored one of its own
 * platform, so an answer from one account can never relabel the other's row.
 *
 * @param stored what the app has loaded before, from the config
 * @param found what the accounts say they play in, if they have been asked —
 *   both platforms' answers together
 * @param activeId the league on screen right now
 */
export function mergeLeagues(
  stored: StoredLeague[],
  found: StoredLeague[],
  activeId: string | null,
): StoredLeague[] {
  const byId = new Map<string, StoredLeague>();
  for (const league of stored) byId.set(league.league_id, league);
  // The account's answer wins on name and season: a league renamed there
  // should not keep showing whatever it was called when this app last loaded
  // it. Only within one platform, though.
  for (const league of found) {
    const known = byId.get(league.league_id);
    if (known !== undefined && known.platform !== league.platform) continue;
    byId.set(league.league_id, league);
  }
  return [...byId.values()].sort((a, b) => {
    if (a.league_id === activeId) return -1;
    if (b.league_id === activeId) return 1;
    if (a.season !== b.season) return b.season.localeCompare(a.season);
    return a.name.localeCompare(b.name);
  });
}

/**
 * Which platform an id belongs to, read off its shape.
 *
 * Yahoo league keys are `<game>.l.<id>` — `449.l.12345` — and Sleeper ids are
 * a long run of digits, so a pasted id says which service it came from
 * without anyone being asked. The backend does the same thing with what is
 * pasted; this is for the handful of places the UI has an id and no league.
 */
export function platformOf(leagueId: string): Platform {
  return leagueId.includes(".l.") ? "yahoo" : "sleeper";
}

/** What the small mark on a league row says, or null for the platform that
 *  needs no explaining because everything else here is one. */
export function platformMark(platform: Platform): string | null {
  return platform === "yahoo" ? "Yahoo" : null;
}

/** Sleeper's league status as a reader would say it, or null for one the
 *  app does not know. */
export function leagueStage(status: string | null): string | null {
  switch (status) {
    case "pre_draft":
      return "draft ahead";
    case "drafting":
      return "drafting now";
    case "in_season":
      return "in season";
    case "complete":
      return "finished";
    default:
      return null;
  }
}

/** What a league row says under its name. */
export function leagueNote(league: StoredLeague, activeId: string | null): string {
  const season = league.season === "" ? "season unknown" : `${league.season} season`;
  const stage = leagueStage(league.status);
  const where = stage === null ? season : `${season} · ${stage}`;
  return league.league_id === activeId ? `${where} · on screen now` : where;
}
