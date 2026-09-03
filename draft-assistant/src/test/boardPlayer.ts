// One player on the draft board, with everything the row reads already filled
// in. Shared between the board's test files so a new field on
// `AvailablePlayer` is added in one place rather than in each of them.

import type { AvailablePlayer } from "../types";

export function boardPlayer(
  id: string,
  name: string,
  position: string,
  over: Partial<AvailablePlayer> = {},
): AvailablePlayer {
  return {
    player_id: id,
    name,
    position,
    team: null,
    bye_week: null,
    points: 100,
    bonus_points: 0,
    vorp: 10,
    tier: 1,
    position_rank: 1,
    overall_rank: 1,
    adp: 20,
    injury_status: null,
    sleeper_pts_ppr: null,
    second_opinion: null,
    survival_next: 0.5,
    ...over,
  };
}
