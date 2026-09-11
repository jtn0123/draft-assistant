/* Draft Assistant companion page. Plain script, no build step: it is
   include_str!'d into the desktop binary and served under a CSP that allows
   nothing inline, so every behaviour lives in this file.
   The pure parts and the node builders live in helpers.js (loaded first) on
   `window.Companion`; this file is the page: boot, sockets, fetches and
   painting. */
(() => {
  "use strict";
  const {
    bindQuestionSuggestions,
    pairBusy,
    pairFailure,
    TOKEN_KEY,
    DEVICE_KEY,
    DEVICE_ID_KEY,
    HOST_KEY,
    TABS,
    LIVE,
    NOTES,
    deviceGuess,
    relativeTime,
    positionClass,
    collapseAgreeing,
    backoffDelay,
    HOST_TIMEOUT,
    timedFetch,
    draftClockFacts,
    initialState,
    reduce,
    syncLine,
    isRevokedClose,
    needsTicker,
    createTicker,
    createHeartbeat,
    clockOffset,
    needsRevive,
    wantsWakeLock,
    createWakeLock,
    registerServiceWorker,
    identityFromAddress,
    rememberIdentityInAddress,
    linkManifest,
    el,
    clear,
    spans,
    buildChatList,
  } = window.Companion;

  function boot() {
    if (!document.getElementById("companion-root")) return;
    const $ = (id) => document.getElementById(id);
    bindQuestionSuggestions(document);
    const models = window.Companion.createMobileModels(document, window);
    window.Companion.createCompact(document, window);
    const restore = window.Companion.createRestore(window, TABS);
    // Pictures are fetched with the pairing token, so the loader is made
    // here and shared with the renderers that draw rows.
    window.Companion.pictures = window.Companion.createPictures(window, () => state.token);
    window.addEventListener("scroll", () => restore.saveScroll(state.tab), { passive: true });
    const alerts = window.Companion.createAlerts(document, window, {
      goNow: () => dispatch({ type: "tab", tab: "now" }),
    });
    let state = initialState();
    let socket = null;
    let attempt = 0;
    const wakeLock = createWakeLock(navigator, document);
    registerServiceWorker(window, navigator);
    linkManifest(document, navigator);
    const request = (url, init) => timedFetch(window, (u, i) => fetch(u, i), url, init);
    // The one pending reconnect. Kept so a wake can cancel it: a timer left
    // running opened a second socket beside the one the wake had just made.
    let reconnectTimer = null;
    const cancelReconnect = () => {
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
    };
    /** Let go of the current socket without its onclose scheduling anything. */
    const detachSocket = () => {
      if (!socket) return;
      socket.onclose = null;
      socket.onmessage = null;
      socket.close();
      socket = null;
    };
    const store = (key, value) => {
      try {
        if (value === null) window.localStorage.removeItem(key);
        else window.localStorage.setItem(key, value);
      } catch {
        return;
      }
    };
    const load = (key) => {
      try {
        return window.localStorage.getItem(key);
      } catch {
        return null;
      }
    };
    const dispatch = (action) => {
      const next = reduce(state, action);
      if (next === state) return;
      state = next;
      if (action.type === "tab") restore.rememberTab(state.tab);
      render();
    };
    const dropToken = () => {
      store(TOKEN_KEY, null);
      cancelReconnect();
      detachSocket();
      dispatch({ type: "unauthorized" });
    };
    /** A read allowed to be absent: 404 only means "nothing loaded there". */
    const read = async (path) => {
      const headers = state.token ? { Authorization: `Bearer ${state.token}` } : {};
      const response = await request(path, { headers });
      if (response.status === 401) dropToken();
      return response.ok ? await response.json() : null;
    };
    async function loadEverything() {
      void read("/api/chat/models")
        .then(models.configure)
        .catch(() => models.configure(null));
      const paths = [
        "/api/state",
        "/api/season",
        "/api/chat?screen=draft",
        "/api/chat?screen=season",
      ];
      try {
        const [draft, season, draftChat, seasonChat] = await Promise.all(paths.map(read));
        if (state.screen !== "app") return;
        dispatch({ type: "draft-updated", payload: draft });
        dispatch({ type: "season-updated", payload: season });
        dispatch({ type: "shared-chat", payload: draftChat });
        dispatch({ type: "shared-chat", payload: seasonChat });
      } catch {
        // The host is away. The socket below is what retries, with backoff,
        // and it is opened whether or not the reads worked: without this a
        // phone that woke with the host down sat on "Reconnecting" for ever.
      } finally {
        connect();
      }
    }
    // An installed copy of the page (see pwa.js) starts with empty storage
    // but an address that says which phone it is; that identity is taken up
    // so the re-pair replaces the browser's entry on the host, and the form
    // offers the name that was used before.
    const carried = identityFromAddress(window.location.search);
    if (carried.deviceId && !load(DEVICE_ID_KEY)) store(DEVICE_ID_KEY, carried.deviceId);
    if (carried.name && !load(DEVICE_KEY)) store(DEVICE_KEY, carried.name);
    $("pair-device").value = load(DEVICE_KEY) || deviceGuess(navigator.userAgent);
    state.hostName = load(HOST_KEY);
    $("pair-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const code = $("pair-code").value.trim();
      const device = $("pair-device").value.trim() || deviceGuess(navigator.userAgent);
      pairBusy($("pair-submit"), true);
      dispatch({ type: "pair-error", message: "" });
      try {
        const response = await request("/api/pair", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            code,
            device_name: device,
            kind: "phone",
            // What makes this a re-pair of the same phone rather than a
            // second one: without it the host would list us twice.
            device_id: load(DEVICE_ID_KEY),
          }),
        });
        const body = await response.json().catch(() => ({}));
        if (response.status !== 200 || !body.token) {
          dispatch({ type: "pair-error", message: pairFailure(response.status) });
          return;
        }
        store(TOKEN_KEY, body.token);
        store(DEVICE_KEY, device);
        if (body.device_id) store(DEVICE_ID_KEY, body.device_id);
        if (body.host_name) store(HOST_KEY, body.host_name);
        rememberIdentityInAddress(window, body.device_id, device);
        dispatch({ type: "paired", token: body.token, hostName: body.host_name });
        await loadEverything();
      } catch {
        dispatch({
          type: "pair-error",
          message: "The host did not answer. Check your connection and try again.",
        });
      } finally {
        pairBusy($("pair-submit"), false);
      }
    });

    // ---- websocket -----------------------------------------------------
    // Pings that have to be answered. A socket the phone's network dropped
    // while the screen was off stays open as far as the page can see, so
    // without this the page sat on a dead connection showing live data.
    const heartbeat = createHeartbeat(window, {
      ping: () => {
        if (socket && socket.readyState === 1) socket.send(JSON.stringify({ type: "ping" }));
      },
      silent: () => {
        // Closing runs `onclose`, which is already the reconnect path.
        if (socket) socket.close();
      },
    });
    function connect() {
      if (!state.token) return;
      // One socket at a time: whatever is pending or open goes first.
      cancelReconnect();
      detachSocket();
      const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${scheme}//${window.location.host}/api/events?token=${state.token}`);
      socket.onopen = () => {
        attempt = 0;
        dispatch({ type: "connection", status: "online" });
        heartbeat.start();
      };
      socket.onmessage = (event) => {
        let frame;
        try {
          frame = JSON.parse(event.data);
        } catch {
          return;
        }
        // `hello` on connect and `pong` on every heartbeat carry the same
        // three facts about the host: its clock, its name, and whether its
        // live sync is actually running. The name is taken from here rather
        // than only from the pairing answer, so renaming the Mac reaches a
        // phone that is already connected.
        if (frame?.type === "pong" || frame?.type === "hello") {
          heartbeat.pong();
          const status = frame.payload ?? {};
          if (status.host_name) store(HOST_KEY, status.host_name);
          dispatch({ type: "clock-offset", offset: clockOffset(status.server_now_ms, Date.now()) });
          dispatch({ type: "host-status", hostName: status.host_name, polling: status.polling });
        } else if (frame?.type === "revoked") dropToken();
        else if (LIVE.includes(frame?.type)) dispatch({ type: frame.type, payload: frame.payload });
      };
      socket.onclose = (event) => {
        heartbeat.stop();
        if (!state.token) return;
        // The host restarted or was revoked: retrying with this token would
        // fail for ever, so the page asks for the code again instead.
        if (isRevokedClose(event)) {
          dropToken();
          return;
        }
        dispatch({ type: "connection", status: "reconnecting" });
        cancelReconnect();
        reconnectTimer = window.setTimeout(connect, backoffDelay(attempt));
        attempt += 1;
      };
    }

    // A phone that was asleep, in flight mode or off the Wi-Fi wakes with a
    // socket the operating system has already thrown away. Nothing errors, so
    // the page has to ask for itself the moment it is looked at again.
    const revive = () => {
      if (!state.token || state.screen !== "app") return;
      if (!needsRevive(socket)) return;
      // Detached, and the pending retry cancelled: either would otherwise
      // open a socket of its own beside the one this wake is about to make.
      cancelReconnect();
      detachSocket();
      heartbeat.stop();
      attempt = 0;
      dispatch({ type: "connection", status: "reconnecting" });
      void loadEverything();
    };
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState !== "visible") return;
      revive();
      // The browser let the lock go when the screen went dark or the tab
      // was left; the page has to ask again the moment it is looked at.
      wakeLock.sync(wantsWakeLock(state));
    });
    window.addEventListener("pageshow", revive);
    window.addEventListener("online", revive);
    // Waiting out a thirty second backoff is the wrong answer to a thumb.
    $("reconnect-pill").addEventListener("click", () => {
      if (!state.token) return;
      cancelReconnect();
      detachSocket();
      heartbeat.stop();
      attempt = 0;
      void loadEverything();
    });

    // ---- chat ----------------------------------------------------------
    /** Which thread the chat block shows: the Week tab is always the season. */
    const chatScreen = () => (state.tab === "week" || !state.draft ? "season" : "draft");
    // One question out at a time; the flag rather than the disabled button,
    // because render() sets the button from the thread's own busy state.
    let asking = false;
    $("chat-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const screen = chatScreen();
      const input = $("chat-input");
      const text = input.value.trim();
      if (!text || asking || !models.available()) return;
      asking = true;
      dispatch({ type: "note", screen, message: null });
      // The question stays in the box until the host has taken it. It used to
      // be cleared first and put back only when the network failed, so a 409,
      // a 500 or a host that never answered lost it with a note beside an
      // empty box.
      try {
        const response = await request("/api/chat", {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${state.token}` },
          body: JSON.stringify({ screen, text, ...models.selection() }),
        });
        if (response.status === 401) {
          dropToken();
          return;
        }
        if (!response.ok) {
          dispatch({
            type: "note",
            screen,
            message: NOTES[response.status] ?? "That did not send.",
          });
          return;
        }
        // Taken. Cleared only if nothing was typed over it meanwhile.
        if (input.value.trim() === text) input.value = "";
      } catch (error) {
        const late = error?.name === HOST_TIMEOUT;
        const message = late ? "The host did not answer in time." : "The host did not answer.";
        dispatch({ type: "note", screen, message });
      } finally {
        asking = false;
      }
    });
    for (const button of $("tabbar").children) {
      button.addEventListener("click", () => dispatch({ type: "tab", tab: button.dataset.tab }));
    }

    // ---- rendering -----------------------------------------------------
    // The pick clock counts down between updates from the host: without this
    // "0:45 left" sat unchanged on screen until the next pick moved, which on
    // a slow pick is the whole minute the number was there to warn about.
    const ticker = createTicker(window, () => render());
    function renderNow() {
      const view = state.draft;
      const strip = clear($("clock-strip"));
      const d = view?.draft;
      spans(strip, ...draftClockFacts(d, Date.now() + state.offset, false));
      window.Companion.renderMobileDraft?.(view, Date.now() + state.offset);
      window.Companion.renderSignals(view);
      const recs = clear($("recs"));
      for (const rec of collapseAgreeing((view?.recommendations ?? []).slice(0, 5))) {
        const card = recs.appendChild(el("li", "card"));
        if (rec.mode) card.appendChild(el("div", "card-mode", rec.mode));
        const head = card.appendChild(el("div", "card-head"));
        head.appendChild(window.Companion.pictures.avatar(rec));
        spans(head, ["name", rec.name], [positionClass(rec.position), rec.position]);
        spans(head, ["muted", rec.team]);
        const survives = rec.survival_next;
        const facts = card.appendChild(el("div", "facts"));
        spans(
          facts,
          [null, `Tier ${rec.tier}`],
          [null, `ADP ${rec.adp === null ? "-" : rec.adp.toFixed(1)}`],
          [
            null,
            typeof survives === "number" &&
              `${Math.round(survives * 100)}% chance available next turn`,
          ],
        );
        const list = card.appendChild(el("ul", "reasons"));
        for (const reason of rec.reasons ?? []) list.appendChild(el("li", null, reason));
      }
      window.Companion.renderRoster(view);
      const size = view ? `${view.data_health.board_size} players on the board` : "";
      $("health").textContent = [size, syncLine(state, Date.now())].filter(Boolean).join(" · ");
    }

    function renderPicks() {
      const list = clear($("picks"));
      const view = state.draft;
      const picks = [...(view?.recent_picks ?? [])].sort((a, b) => b.pick_no - a.pick_no);
      const mySlot = view?.draft?.my_slot ?? null;
      for (const pick of picks.slice(0, 25)) {
        const row = list.appendChild(el("li", pick.slot === mySlot ? "row is-mine" : "row"));
        spans(row, ["pick-no", `${pick.round}.${pick.pick_no}`]);
        row.appendChild(window.Companion.pictures.avatar(pick));
        spans(row, ["name", pick.name]);
        spans(row, [positionClass(pick.position), pick.position]);
        spans(row, ["muted", pick.slot_name || `Slot ${pick.slot}`]);
      }
      if (!picks.length) list.appendChild(el("li", "muted", "No picks yet."));
      window.Companion.renderAvailable(view);
    }

    // Where the chat block sits and which thread its list was built from.
    // Both are checked before touching the DOM: this runs on every clock
    // tick, and moving the block or rebuilding the list each second took the
    // focus off the input while someone was typing on a phone.
    let chatShownIn = null;
    let chatBuiltFrom = { screen: null, thread: undefined, fresh: true };
    let chatEntries = 0;
    function renderChat() {
      const screen = chatScreen();
      const shownIn = state.tab === "week" ? "tab-week" : "tab-chat";
      if (chatShownIn !== shownIn) {
        $(shownIn).appendChild($("chat-block"));
        chatShownIn = shownIn;
      }
      const thread = state.chat[screen];
      const list = $("chat-list");
      const now = Date.now();
      const same =
        !chatBuiltFrom.fresh && chatBuiltFrom.screen === screen && chatBuiltFrom.thread === thread;
      if (same) {
        // Only the "4m ago" lines move on a tick; the nodes stay put.
        for (const when of list.querySelectorAll("[data-at]")) {
          when.textContent = relativeTime(Number(when.dataset.at), now);
        }
      } else {
        const grew = (thread?.entries ?? []).length > chatEntries;
        chatEntries = (thread?.entries ?? []).length;
        buildChatList(list, thread?.entries ?? [], now, Boolean(thread?.busy));
        // A new answer lands below the fold on a long thread; bring it up.
        if (grew && !$(shownIn).hidden) list.lastElementChild?.scrollIntoView?.({ block: "end" });
        chatBuiltFrom = { screen, thread, fresh: false };
      }
      $("chat-note").hidden = !state.note[screen];
      $("chat-note").textContent = state.note[screen] ?? "";
      const busy = Boolean(thread?.busy);
      $("chat-input").disabled = busy;
      $("chat-send").disabled = busy || !models.available();
      models.setBusy(busy);
      $("chat-send").textContent = busy ? "Answering…" : "Send";
    }

    function render() {
      $("pair-screen").hidden = state.screen !== "pair";
      $("app-screen").hidden = state.screen !== "app";
      $("tabbar").hidden = state.screen !== "app";
      $("reconnect-pill").hidden = !(state.screen === "app" && state.connection === "reconnecting");
      $("pair-host").hidden = !state.hostName;
      $("pair-host").textContent = state.hostName ? `Hosted by ${state.hostName}` : "";
      $("pair-error").hidden = !state.pairError;
      $("pair-error").textContent = state.pairError ?? "";
      ticker.sync(needsTicker(state));
      wakeLock.sync(wantsWakeLock(state));
      alerts.observe(state, Date.now() + state.offset);
      if (state.screen === "pair") return;
      for (const button of $("tabbar").children) {
        if (button.dataset.tab === "week") button.hidden = !state.season;
        if (button.dataset.tab === state.tab) button.setAttribute("aria-current", "page");
        else button.removeAttribute("aria-current");
      }
      for (const tab of TABS) $(`tab-${tab}`).hidden = tab !== state.tab;
      if (state.tab === "now") renderNow();
      else if (state.tab === "picks") renderPicks();
      else if (state.tab === "chat") renderChat();
      else if (state.tab === "week") {
        window.Companion.renderWeek(state);
        renderChat();
      }
      if (state.draft || state.season) restore.restoreScroll(state.tab);
    }

    const saved = load(TOKEN_KEY);
    if (saved) {
      // Back on the tab this phone was on, unless it was the Week tab: that
      // one is hidden until the season snapshot arrives and picks itself.
      const tab = restore.tab();
      state = { ...state, token: saved, screen: "app", tab: tab && tab !== "week" ? tab : "now" };
      // A phone paired before the address carried its identity gets it now,
      // so an install made today still pairs as the same phone.
      rememberIdentityInAddress(window, load(DEVICE_ID_KEY), load(DEVICE_KEY));
      void loadEverything();
    }
    render();
    $("pair-form").dataset.ready = "true";
    $("pair-submit").disabled = false;
    $("boot-status").hidden = true;
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
