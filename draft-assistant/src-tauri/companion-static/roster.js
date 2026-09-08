/* Draft Assistant companion page: your roster on the Now tab. Who you have
   taken so far, in pick order, and the starting slots still open under it.
   Loaded after helpers.js and before app.js; adds to `window.Companion`. */
(() => {
  "use strict";

  /** "1.2": the round and the overall pick, the way the desktop reads it. */
  const pickLabel = (p) => `${p.round ?? "-"}.${p.pick_no ?? "-"}`;
  /** "Still to fill: 1 QB, 2 RB" from `[[slot, count]]`, in the host's slot
   *  order; an empty string when every starter is covered. */
  const openLine = (open) => {
    const parts = (Array.isArray(open) ? open : [])
      .filter((pair) => Array.isArray(pair) && pair[1] > 0)
      .map(([slot, count]) => `${count} ${slot}`);
    return parts.length ? `Still to fill: ${parts.join(", ")}` : "";
  };
  /** The drafted players by pick number, so a keeper taken in round eight
   *  sits under the round-one pick rather than wherever the host listed it. */
  const inPickOrder = (players) =>
    [...(Array.isArray(players) ? players : [])].sort(
      (a, b) => (a?.pick_no ?? 0) - (b?.pick_no ?? 0),
    );
  /** The player's picture from the pictures lane, or null when that lane is
   *  not loaded or hands back something that is not a node. */
  const avatarFor = (p) => {
    const node = window.Companion.pictures?.avatar?.(p);
    return node && node.nodeType === 1 ? node : null;
  };
  const buildRow = (p) => {
    const { el, spans, positionClass } = window.Companion;
    const row = el("div", "roster-row");
    const avatar = avatarFor(p);
    if (avatar) row.appendChild(avatar);
    spans(row, ["pick-no", pickLabel(p)], ["name", p.name]);
    spans(row, [positionClass(p.position), p.position], ["muted", p.team]);
    if (p.is_keeper) row.appendChild(el("span", "kind", "keeper"));
    return row;
  };

  /** What the roster was last built from: renderNow runs on every clock
   *  second, and the roster changes once a round at most. */
  let signature = "";

  /** Paints `#roster` and the open-starters line under it, rebuilding only
   *  when a pick or a slot has changed since the last paint. */
  const renderRoster = (view) => {
    const { el, clear } = window.Companion;
    const roster = document.getElementById("roster");
    const open = document.getElementById("roster-open");
    if (!roster || !open) return;
    const players = inPickOrder(view?.my_roster?.players);
    const starters = view?.my_roster?.open_starters ?? [];
    const next = JSON.stringify([
      players.map((p) => [
        p.player_id,
        p.pick_no,
        p.round,
        p.is_keeper,
        p.name,
        p.position,
        p.team,
      ]),
      starters,
    ]);
    if (next === signature) return;
    signature = next;
    clear(roster);
    for (const p of players) roster.appendChild(buildRow(p));
    if (!players.length) roster.appendChild(el("p", "muted", "Nothing drafted yet."));
    open.textContent = openLine(starters);
  };

  window.Companion = { ...window.Companion, renderRoster };
})();
