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

  window.Companion = {
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
