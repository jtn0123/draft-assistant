/* Draft Assistant companion page: the best-available list on the Picks tab.
   Who is still on the board, ranked, with a row of position chips to narrow
   it and a button under it to see past the first dozen. Loaded after
   helpers.js and before app.js; adds to `window.Companion`. */
(() => {
  "use strict";
  /** Where the chosen chip lives between reloads: a phone that was on "RB"
   *  when the screen locked should still be on "RB" when it wakes. */
  const FILTER_KEY = "da.companion.available-filter";
  /** Chip order. Not alphabetical: it is the order a draft is thought about. */
  const POSITIONS = ["QB", "RB", "WR", "TE", "K", "DEF"];
  const ALL = "All";
  /** How many rows show before "Show all": a phone screen's worth, so the
   *  board is a glance rather than a scroll until it is asked to be one. */
  const LIMIT = 12;

  /** The positions on the board, in chip order, skipping the ones no player
   *  holds: a chip that filters down to nothing is a dead button. */
  const availablePositions = (list) => {
    const present = new Set((list ?? []).map((p) => p?.position));
    return POSITIONS.filter((pos) => present.has(pos));
  };
  /** The top `limit` of the board, or of one position; `Infinity` for all
   *  of it. The list arrives sorted by overall rank, so this keeps the
   *  host's order. */
  const bestAvailable = (list, filter, limit = LIMIT) => {
    const rows =
      filter && filter !== ALL ? (list ?? []).filter((p) => p?.position === filter) : list;
    return (rows ?? []).slice(0, limit);
  };

  const readFilter = () => {
    try {
      return window.localStorage.getItem(FILTER_KEY) || ALL;
    } catch {
      return ALL;
    }
  };
  const remember = (filter) => {
    try {
      window.localStorage.setItem(FILTER_KEY, filter);
    } catch {
      /* Private browsing: the choice lasts the session. */
    }
  };

  const factsLine = (p) => {
    const facts = [`Tier ${p.tier ?? "-"}`];
    facts.push(typeof p.adp === "number" ? `ADP ${p.adp.toFixed(1)}` : "ADP -");
    if (p.bye_week != null) facts.push(`Bye ${p.bye_week}`);
    if (typeof p.survival_next === "number")
      facts.push(`${Math.round(p.survival_next * 100)}% next turn`);
    return facts.join(" · ");
  };
  /** The player's picture from the pictures lane, or null when that lane is
   *  not loaded or hands back something that is not a node. */
  const avatarFor = (p) => {
    const node = window.Companion.pictures?.avatar?.(p);
    return node && node.nodeType === 1 ? node : null;
  };
  const buildRow = (p) => {
    const { el, spans, positionClass } = window.Companion;
    const row = el("li", "row");
    const avatar = avatarFor(p);
    if (avatar) row.appendChild(avatar);
    spans(row, ["rank", String(p.overall_rank ?? "")], ["name", p.name]);
    spans(row, [positionClass(p.position), p.position], ["muted", p.team]);
    if (typeof p.injury_status === "string" && p.injury_status) {
      row.appendChild(el("span", "injury", p.injury_status));
    }
    row.appendChild(el("div", "facts", factsLine(p)));
    return row;
  };

  let chosen = null;
  let lastView = null;
  /** The chip the list was last opened all the way out for, or null. Held as
   *  a name rather than a flag so a tap on another chip, or a remembered
   *  chip falling back to All, folds the list back to its first dozen. */
  let expandedFor = null;
  /** What the chips and the list were last built from. Two signatures, one
   *  each: a tap on a chip rebuilds the list and only re-presses the chips,
   *  so the button under the thumb stays the same node. */
  let chipSignature = "";
  let listSignature = "";
  let bound = null;

  /** Tapping a chip narrows the list at once, from the view last rendered.
   *  Bound once per chip strip; the strip is the same node for the life of
   *  the page, but a test rebuilds the body between boots. */
  const bindChips = (strip) => {
    if (bound === strip) return;
    bound = strip;
    strip.addEventListener("click", (event) => {
      const button = event.target.closest?.("button[data-filter]");
      if (!button) return;
      chosen = button.dataset.filter;
      expandedFor = null;
      remember(chosen);
      renderAvailable(lastView);
    });
  };

  const paintChips = (strip, positions, filter) => {
    const { el, clear } = window.Companion;
    const signature = JSON.stringify(positions);
    if (signature !== chipSignature) {
      chipSignature = signature;
      clear(strip);
      for (const pos of [ALL, ...positions]) {
        const button = strip.appendChild(el("button", null, pos));
        button.type = "button";
        button.dataset.filter = pos;
      }
    }
    for (const button of strip.querySelectorAll("button[data-filter]")) {
      button.setAttribute("aria-pressed", String(button.dataset.filter === filter));
    }
  };

  /** The "Show all 31" / "Show fewer" button after the list. Made afresh on
   *  each list rebuild and left out when the whole list already fits. */
  const paintShowAll = (list, filter, total, expanded) => {
    const { el } = window.Companion;
    const old = list.nextElementSibling;
    if (old?.classList?.contains("show-all")) old.remove();
    if (total <= LIMIT) return;
    const button = el("button", "show-all", expanded ? "Show fewer" : `Show all ${total}`);
    button.type = "button";
    button.setAttribute("aria-expanded", String(expanded));
    button.addEventListener("click", () => {
      expandedFor = expanded ? null : filter;
      renderAvailable(lastView);
    });
    list.insertAdjacentElement("afterend", button);
  };

  /** Called from renderPicks on every paint of the Picks tab, which includes
   *  each one-second clock tick: the work before the signature check is a
   *  filter over the board and nothing else, and the DOM is only touched when
   *  the rows it would show, or the chip pressed, are not the ones showing. */
  const renderAvailable = (view) => {
    const { el, clear } = window.Companion;
    const strip = document.getElementById("available-filter");
    const list = document.getElementById("available");
    if (!strip || !list) return;
    lastView = view;
    bindChips(strip);
    if (chosen === null) chosen = readFilter();
    const board = Array.isArray(view?.available) ? view.available : [];
    const positions = availablePositions(board);
    // A remembered chip for a position nobody on the board holds any more
    // (the last kicker went) falls back to the whole board rather than an
    // empty list under a chip that is not there to un-press.
    const filter = chosen === ALL || positions.includes(chosen) ? chosen : ALL;
    paintChips(strip, positions, filter);
    const all = bestAvailable(board, filter, Infinity);
    const expanded = expandedFor === filter;
    const rows = expanded ? all : all.slice(0, LIMIT);
    // The total rides along so a thirteenth player arriving under a folded
    // list brings the button with it, even though the rows are the same.
    const signature = JSON.stringify([
      filter,
      expanded,
      all.length,
      rows.map((p) => [
        p.player_id,
        p.overall_rank,
        p.tier,
        p.adp,
        p.injury_status,
        p.survival_next,
      ]),
    ]);
    if (signature === listSignature) return;
    listSignature = signature;
    clear(list);
    for (const p of rows) list.appendChild(buildRow(p));
    if (!rows.length) list.appendChild(el("li", "muted", "No players on the board yet."));
    paintShowAll(list, filter, all.length, expanded);
  };

  window.Companion = {
    ...window.Companion,
    availablePositions,
    bestAvailable,
    renderAvailable,
  };
})();
