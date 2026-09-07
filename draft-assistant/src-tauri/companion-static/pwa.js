/* Draft Assistant companion page, the installed-app half.
   The screen wake lock, the service worker registration and manifest link
   that let the browser offer Add to Home Screen, and the identity the page
   carries in its own address so an installed copy pairs as the same phone.
   The first two need a secure context: they do nothing on the plain http LAN
   address and come alive on the https tailnet one. Loaded after clock.js and
   before app.js; it adds to `window.Companion`. */
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
   *  `sync(true)` requests a new one. Only while the page is visible, though:
   *  a hidden page cannot hold the lock, so asking from one is refused, and
   *  the pick clock repaints every second, which made a phone with the screen
   *  off ask and be refused once a second until the draft ended. The request
   *  waits for the `visibilitychange` back to visible, which app.js hands to
   *  `sync` as it always did. A browser with no `wakeLock` (the plain http
   *  address, an older phone) makes every call a no-op. */
  const createWakeLock = (nav, doc) => {
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
        if (doc && doc.visibilityState !== "visible") return;
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

  /** The query keys the page's identity travels under in its own address. */
  const DEVICE_PARAM = "device";
  const NAME_PARAM = "name";

  /** The device id and name carried in the page's address, if any.
   *
   *  iOS gives a web app added to the home screen a storage of its own, so
   *  nothing Safari saved (token, device id, name) reaches it: the installed
   *  app opened on the pairing screen and, with no id to send, the host
   *  listed it as "iPhone 2" beside the Safari pairing. The token is a
   *  secret and stays out of the address; the id and name are not, and are
   *  enough for the re-pair to replace the old entry rather than add one. */
  const identityFromAddress = (search) => {
    const params = new URLSearchParams(search || "");
    const deviceId = params.get(DEVICE_PARAM) || null;
    const name = params.get(NAME_PARAM) || null;
    return { deviceId, name };
  };

  /** The page's address with the identity written into its query, and the
   *  rest of it (path, other params, hash) left as it was. */
  const addressWithIdentity = (loc, deviceId, name) => {
    const params = new URLSearchParams(loc.search || "");
    if (deviceId) params.set(DEVICE_PARAM, deviceId);
    if (name) params.set(NAME_PARAM, name);
    const query = params.toString();
    return `${loc.pathname || "/"}${query ? `?${query}` : ""}${loc.hash || ""}`;
  };

  /** Put the identity in the address bar without a navigation, so it is what
   *  Add to Home Screen bookmarks. Quiet when the window has no history to
   *  rewrite (the test's bare context) or the address already says it. */
  const rememberIdentityInAddress = (win, deviceId, name) => {
    if (!deviceId || typeof win?.history?.replaceState !== "function") return;
    const next = addressWithIdentity(win.location, deviceId, name);
    const current = `${win.location.pathname || "/"}${win.location.search || ""}${win.location.hash || ""}`;
    if (next !== current) win.history.replaceState(null, "", next);
  };

  /** Whether this browser installs web apps with a storage of their own,
   *  which today means iOS: `navigator.standalone` exists nowhere else. On
   *  such a browser the manifest is left out, because a manifest names a
   *  `start_url` and the installed app opens there instead of at the address
   *  that carries the identity; the legacy meta tags in index.html give iOS
   *  everything the manifest would have (standalone display, title, icon).
   *  Everywhere else the manifest is what makes the page installable, and
   *  the installed app shares the browser's storage anyway. */
  const installsWithOwnStorage = (nav) => typeof nav?.standalone === "boolean";

  /** Add the manifest link for browsers that share storage with the app
   *  they install; returns whether it was added. */
  const linkManifest = (doc, nav) => {
    if (!doc?.head || installsWithOwnStorage(nav)) return false;
    const link = doc.createElement("link");
    link.rel = "manifest";
    link.href = "/static/manifest.webmanifest";
    doc.head.appendChild(link);
    return true;
  };

  window.Companion = {
    ...window.Companion,
    LIVE_DRAFT,
    wantsWakeLock,
    createWakeLock,
    registerServiceWorker,
    identityFromAddress,
    addressWithIdentity,
    rememberIdentityInAddress,
    installsWithOwnStorage,
    linkManifest,
  };
})();
