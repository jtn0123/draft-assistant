/* Draft Assistant companion page — the installed-app half.
   The screen wake lock, and the service worker registration that lets the
   browser offer Add to Home Screen. Both need a secure context: they do
   nothing on the plain http LAN address and come alive on the https tailnet
   one. Loaded after clock.js and before app.js; it adds to `window.Companion`. */
(() => {
  "use strict";
  /** Draft statuses during which the screen is held awake. A paused draft is
   *  still a draft everyone is sitting at; a finished one is not. */
  const LIVE_DRAFT = ["drafting", "paused"];

  /** Whether the screen should be held awake: in the app, connected to the
   *  host, with a draft under way. Pure, so a test can ask it directly. */
  const wantsWakeLock = (state) =>
    Boolean(
      state &&
      state.screen === "app" &&
      state.connection === "online" &&
      LIVE_DRAFT.includes(state.draft?.draft?.status),
    );

  /** One screen wake lock, asked for and let go of as the page's state says.
   *
   *  The browser releases it by itself when the page is hidden, so the page
   *  asks again on every render and every wake rather than once: a sentinel
   *  that has been released is forgotten on its `release` event and the next
   *  `sync(true)` requests a new one. A browser with no `wakeLock` (the plain
   *  http address, an older phone) makes every call a no-op. */
  const createWakeLock = (nav) => {
    let sentinel = null;
    let pending = null;
    const release = () => {
      const held = sentinel;
      sentinel = null;
      if (held) held.release().catch(() => {});
    };
    return {
      sync: (wanted) => {
        if (!nav?.wakeLock?.request) return;
        if (!wanted) {
          release();
          return;
        }
        if (sentinel || pending) return;
        pending = nav.wakeLock
          .request("screen")
          .then((held) => {
            sentinel = held;
            held.addEventListener?.("release", () => {
              if (sentinel === held) sentinel = null;
            });
          })
          .catch(() => {})
          .finally(() => {
            pending = null;
          });
      },
      held: () => sentinel !== null,
    };
  };

  /** Register the do-nothing service worker, which is what makes the page
   *  installable. Only over https: registration throws anywhere else, and
   *  the LAN address is not a place the page can be installed from. */
  const registerServiceWorker = (win, nav) => {
    if (!win?.isSecureContext || !nav?.serviceWorker?.register) return;
    nav.serviceWorker.register("/static/sw.js", { scope: "/" }).catch(() => {});
  };

  window.Companion = {
    ...window.Companion,
    LIVE_DRAFT,
    wantsWakeLock,
    createWakeLock,
    registerServiceWorker,
  };
})();
