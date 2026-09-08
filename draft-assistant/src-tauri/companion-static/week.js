/* Draft Assistant companion page, the Week tab.
   Moved out of app.js, which sits at the source size cap; it paints the
   season header and the start/sit calls from the page's state. Loaded after
   helpers.js and before app.js; it adds to `window.Companion`. */
(() => {
  "use strict";
  const renderWeek = (state) => {
    const { el, clear, spans } = window.Companion;
    const $ = (id) => document.getElementById(id);
    const view = state.season;
    const header = clear($("week-header"));
    if (!view) return;
    const head = view.header ?? {};
    const live = view.live?.totals;
    const projected =
      typeof head.my_projected === "number" &&
      `Projected ${head.my_projected.toFixed(1)} - ${head.opp_projected.toFixed(1)}`;
    spans(
      header,
      ["headline", `Week ${view.week}`],
      [null, `${view.matchup?.my_name ?? "You"} vs ${head.opponent_name ?? "-"}`],
      ["mine", live && `${live.my_live_points.toFixed(1)} - ${live.opp_live_points.toFixed(1)}`],
      ["muted", projected],
    );
    const behind = state.seasonHealth?.consecutive_failures ?? 0;
    if (behind) spans(header, ["muted", `${behind} failed syncs`]);
    const calls = clear($("week-calls"));
    for (const call of view.calls ?? []) {
      const row = calls.appendChild(el("li", "row"));
      spans(row, ["pick-no", call.slot], ["name", `Start ${call.player_in}`]);
      spans(row, ["muted", `over ${call.player_out}`], ["mine", `+${call.gain.toFixed(1)}`]);
      spans(row, ["muted", call.why]);
    }
    if (!(view.calls ?? []).length) {
      calls.appendChild(el("li", "muted", "The lineup you have set is already the best one."));
    }
  };

  window.Companion = { ...window.Companion, renderWeek };
})();
