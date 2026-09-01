/**
 * What the win probability is worth, in one line.
 *
 * The number comes from `season_odds::win_probability`, whose spread is
 * `season_spread`: every starter's own week-to-week spread, measured from a
 * full season of player-weeks, plus a fitted hedge for the upsets a normal
 * curve is too thin to hold. Both were checked by replaying that season
 * through the shipped model — `cargo run --bin backtest` — rather than
 * guessed. Shown next to the odds so the number is read as a model, not a
 * promise.
 */
export const ODDS_NOTE = "calibrated on 2025 season player-weeks";
