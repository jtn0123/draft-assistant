/* Draft Assistant companion page: the one-line signals above Recommended.
   A position run, if one is on, and any tier about to run out, worded the
   way the desktop's Tier alerts panel words them. Loaded after helpers.js
   and before app.js; adds to `window.Companion`. */
(() => {
  "use strict";
  /** A tier with this many or fewer left is worth a line; more is noise. */
  const TIER_LOW = 3;

  /** "RB run in progress: 4 of the last 6", as the desktop puts it. */
  const runLine = (run) =>
    run && typeof run === "object" && run.position
      ? `${run.position} run in progress: ${run.count} of the last ${run.window}`
      : null;
  /** "TE tier 3: 1 left" for each position whose top tier is nearly gone. */
  const tierLines = (alerts) =>
    (Array.isArray(alerts) ? alerts : [])
      .filter((a) => typeof a?.players_left === "number" && a.players_left <= TIER_LOW)
      .map((a) => `${a.position} tier ${a.tier}: ${a.players_left} left`);
  /** The whole line, run first; empty when there is nothing worth saying. */
  const signalsLine = (view) =>
    [runLine(view?.position_run), ...tierLines(view?.tier_alerts)].filter(Boolean).join(" · ");

  /** The line last painted: renderNow runs on every clock second, and the
   *  signals change once a pick at most. */
  let painted = null;

  /** Fills `#signals` and shows it, or hides it when the line is empty. */
  const renderSignals = (view) => {
    const node = document.getElementById("signals");
    if (!node) return;
    const line = signalsLine(view);
    if (line === painted) return;
    painted = line;
    node.textContent = line;
    node.hidden = !line;
  };

  window.Companion = { ...window.Companion, signalsLine, renderSignals };
})();
