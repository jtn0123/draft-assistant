/**
 * What the win probability is worth, in one line.
 *
 * The number comes from `matchup::preview`, whose spread was fitted on last
 * season's 98 games (`src-tauri/src/bin/backtest.rs`): every starter's spread
 * measured from 1,746 player-weeks, then a hedge for the upsets a normal
 * curve is too thin to hold. Of the games it called 70% or better, 81% won.
 * Shown next to the odds so the number is read as a model, not a promise.
 */
export const ODDS_NOTE = "calibrated on last season · its 70%+ calls went 81%";
