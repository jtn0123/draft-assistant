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

  window.Companion = {
    ...window.Companion,
    TICK_MS,
    REVOKED_CLOSE,
    isRevokedClose,
    needsTicker,
    createTicker,
  };
})();
