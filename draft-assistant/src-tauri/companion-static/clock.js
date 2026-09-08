/* Draft Assistant companion page — the ticking half.
   Everything about time passing on its own: whether the pick clock should be
   counting, the one-second ticker that makes it count, and how the page tells
   a socket closed because the host forgot this device from any other close.
   Loaded after helpers.js and before app.js; it adds to `window.Companion`. */
(() => {
  "use strict";
  /** How often the clock and the "4m ago" lines are repainted. */
  const TICK_MS = 1000;
  /** The close code the host sends when the token is no good any more. */
  const REVOKED_CLOSE = 4401;

  /** Whether a socket closed because this device is no longer paired.
   *  A browser is told nothing about a failed WebSocket handshake, so the
   *  close code is the only way the page can tell "the host restarted" from
   *  "the Wi-Fi dropped", and the two want opposite reactions: pair again,
   *  or keep retrying. */
  const isRevokedClose = (event) => Number(event?.code) === REVOKED_CLOSE;

  /** Whether anything on screen changes with the wall clock: a pick clock
   *  running, or chat entries whose "just now" goes stale. */
  const needsTicker = (state) => {
    if (!state || state.screen !== "app") return false;
    if (typeof state.draft?.draft?.clock_deadline_ms === "number") return true;
    return Object.values(state.chat ?? {}).some((thread) => (thread?.entries ?? []).length > 0);
  };

  /** A one-second repaint that runs only while something needs it.
   *  `timers` is whatever owns setInterval — the window on a phone, the fake
   *  clock in a test — so nothing in here reaches for a global. */
  const createTicker = (timers, onTick) => {
    let handle = null;
    const stop = () => {
      if (handle !== null) timers.clearInterval(handle);
      handle = null;
    };
    return {
      /** Start ticking, or stop: called after every render, so a draft that
       *  ends takes its interval down with it rather than repainting a dead
       *  clock for as long as the page is open. */
      sync: (needed) => {
        if (needed && handle === null) handle = timers.setInterval(onTick, TICK_MS);
        else if (!needed) stop();
      },
      stop,
      running: () => handle !== null,
    };
  };

  /** How often the page pings the host, and how many unanswered pings it
   *  puts up with before it treats the socket as dead. */
  const HEARTBEAT_MS = 25000;
  const MISSED_PONGS = 2;

  /** A ping that has to be answered.
   *
   *  The page used to ping and never read the reply, so a socket the phone's
   *  network had quietly dropped — asleep in a pocket, off a lift, a router
   *  that forgot the connection — stayed `readyState === 1` for ever. Nothing
   *  arrived and nothing errored: the page looked live and was not. Two
   *  unanswered pings is the whole of "this socket is gone".
   *
   *  `timers` is whatever owns setInterval, so a test can drive it by hand. */
  const createHeartbeat = (timers, { ping, silent, intervalMs = HEARTBEAT_MS }) => {
    let handle = null;
    let unanswered = 0;
    const stop = () => {
      if (handle !== null) timers.clearInterval(handle);
      handle = null;
    };
    return {
      start: () => {
        stop();
        unanswered = 0;
        handle = timers.setInterval(() => {
          if (unanswered >= MISSED_PONGS) {
            stop();
            silent();
            return;
          }
          unanswered += 1;
          ping();
        }, intervalMs);
      },
      /** The host answered: whatever is in flight is accounted for. */
      pong: () => {
        unanswered = 0;
      },
      stop,
      running: () => handle !== null,
      unanswered: () => unanswered,
    };
  };

  /** How far the host's clock is ahead of this phone's, in milliseconds.
   *  Added to `Date.now()` wherever the page counts a host deadline down, so
   *  a phone whose own clock is minutes out does not show a pick timer that
   *  is minutes wrong. A host that says nothing sensible means no offset. */
  const clockOffset = (serverNowMs, localNowMs) =>
    typeof serverNowMs === "number" && isFinite(serverNowMs) ? serverNowMs - localNowMs : 0;

  /** Whether the page has to build a socket again: it has none, or the one it
   *  has is not open. A phone coming back from sleep asks this before it
   *  throws a working connection away. */
  const needsRevive = (socket) => !socket || socket.readyState !== 1;

  const pickLabel = (pick, teams) =>
    `${Math.floor((pick - 1) / teams) + 1}.${String(((pick - 1) % teams) + 1).padStart(2, "0")}`;
  const waiting = (draft) =>
    draft.status === "pre_draft" &&
    (draft.total_picks_made ?? 0) <= (draft.keeper_picks?.length ?? 0);
  /** Same plain-snake baseline as desktop; overrides cover trades and 3RR. */
  const mobilePickQueue = (view) => {
    const d = view?.draft;
    if (!d || d.status === "complete" || d.is_auction || !Number.isInteger(d.teams) || d.teams < 1)
      return [];
    const names = new Map((view.rosters ?? []).map((r) => [r.slot, r.display_name]));
    const keepers = new Set(d.keeper_picks ?? []);
    const queue = [];
    for (let pick = d.current_pick; pick <= d.teams * d.rounds && queue.length < 24; pick++) {
      if (keepers.has(pick)) continue;
      const round = Math.floor((pick - 1) / d.teams) + 1;
      const index = (pick - 1) % d.teams;
      const slot =
        d.pick_slot_overrides?.[String(pick)] ?? (round % 2 ? index + 1 : d.teams - index);
      queue.push({
        pick,
        slot,
        label: pickLabel(pick, d.teams),
        name: names.get(slot) || `Slot ${slot}`,
        mine: slot === d.my_slot,
      });
    }
    return queue;
  };
  let queueSignature = "";
  const renderMobileDraft = (view, now) => {
    const { el, clear, formatClock } = window.Companion;
    const strip = document.getElementById("clock-strip");
    if (!strip) return;
    const d = view?.draft;
    // renderNow rebuilds the clock strip; the queue stays mounted while time ticks.
    strip.querySelector(".mobile-timer")?.remove();
    if (
      d &&
      !waiting(d) &&
      !d.paused &&
      d.status !== "paused" &&
      d.status !== "complete" &&
      !d.is_auction
    ) {
      const timer = strip.appendChild(el("div", "mobile-timer"));
      if (typeof d.clock_deadline_ms === "number") {
        timer.appendChild(el("span", "mobile-timer-label", "Time left"));
        const value = timer.appendChild(
          el("strong", "mobile-timer-value", formatClock(d.clock_deadline_ms, now)),
        );
        value.setAttribute("role", "timer");
        const seconds = Math.max(0, Math.ceil((d.clock_deadline_ms - now) / 1000));
        timer.classList.toggle("urgent", seconds <= 15);
        if (d.pick_timer > 0) {
          const progress = timer.appendChild(el("progress"));
          progress.max = d.pick_timer;
          progress.value = Math.min(d.pick_timer, seconds);
          progress.setAttribute("aria-label", "Pick time remaining");
        }
      } else timer.appendChild(el("span", "muted small", "Waiting for the live pick clock"));
    } else if (d?.pick_timer > 0 && waiting(d)) {
      strip.appendChild(el("div", "mobile-timer muted small", `${d.pick_timer} seconds per pick`));
    }
    let panel = document.getElementById("mobile-up-next");
    if (!panel) {
      panel = el("section", "mobile-up-next");
      panel.id = "mobile-up-next";
      panel.setAttribute("aria-label", "Upcoming draft picks");
      strip.after(panel);
      queueSignature = "";
    }
    const signature = JSON.stringify([d, view?.rosters]);
    if (signature === queueSignature) return;
    queueSignature = signature;
    const expanded = panel.querySelector("details")?.open ?? false;
    clear(panel);
    panel.hidden = !d || d.status === "complete" || Boolean(d.is_auction);
    if (panel.hidden) return;
    panel.appendChild(el("h2", null, "Who’s next"));
    const orderKnown = (view.rosters ?? []).some((r) => r.display_name);
    if (waiting(d) && !orderKnown) {
      panel.appendChild(el("p", "muted small", "Waiting for the draft order to be posted."));
      return;
    }
    if (d.paused || d.status === "paused")
      panel.appendChild(el("p", "muted small", "Upcoming order · draft paused"));
    else if (d.picks_until_mine != null)
      panel.appendChild(
        el(
          "p",
          "mobile-your-turn",
          d.picks_until_mine === 0 && !waiting(d)
            ? "You’re up now"
            : `${d.picks_until_mine} picks until your turn`,
        ),
      );
    const queue = mobilePickQueue(view);
    const list = el("ol", "mobile-pick-queue");
    const add = (parent, entry) => {
      const row = parent.appendChild(el("li", entry.mine ? "queue-pick is-mine" : "queue-pick"));
      row.appendChild(el("span", "queue-number", entry.label));
      row.appendChild(el("span", "queue-name", entry.name));
      if (entry.mine) row.appendChild(el("span", "queue-you", "You"));
    };
    // On your own pick the recommendations matter more than the queue, so
    // fewer rows sit between the clock and them.
    const shown = d.is_my_pick && !waiting(d) ? 2 : 4;
    queue.slice(0, shown).forEach((entry) => add(list, entry));
    panel.appendChild(list);
    if (queue.length > shown) {
      const more = panel.appendChild(el("details", "queue-more"));
      more.open = expanded;
      more.appendChild(el("summary", null, `Show ${queue.length - shown} more upcoming picks`));
      const rest = more.appendChild(el("ol", "mobile-pick-queue"));
      queue.slice(shown).forEach((entry) => add(rest, entry));
    }
    if (d.my_next_picks?.length) {
      panel.appendChild(
        el(
          "p",
          "mobile-my-picks",
          `Your next picks: ${d.my_next_picks
            .slice(0, 4)
            .map((pick) => pickLabel(pick, d.teams))
            .join(" · ")}`,
        ),
      );
    } else if (d.seat_note) panel.appendChild(el("p", "muted small", d.seat_note));
  };

  window.Companion = {
    mobilePickQueue,
    renderMobileDraft,
    ...window.Companion,
    TICK_MS,
    REVOKED_CLOSE,
    HEARTBEAT_MS,
    MISSED_PONGS,
    isRevokedClose,
    needsTicker,
    createTicker,
    createHeartbeat,
    clockOffset,
    needsRevive,
  };
})();
