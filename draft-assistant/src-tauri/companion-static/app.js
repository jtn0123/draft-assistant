/* Draft Assistant companion page. Plain script, no build step: it is
   include_str!'d into the desktop binary and served under a CSP that allows
   nothing inline, so every behaviour lives in this file.
   The pure parts live in helpers.js (loaded first) on `window.Companion`;
   this file is the DOM: boot, sockets, fetches and painting. */
(() => {
  "use strict";
  const {
    TOKEN_KEY,
    DEVICE_KEY,
    DEVICE_ID_KEY,
    HOST_KEY,
    REVOKED,
    TABS,
    LIVE,
    NOTES,
    deviceGuess,
    relativeTime,
    positionClass,
    collapseAgreeing,
    backoffDelay,
    formatCost,
    formatClock,
    parseMarkdown,
    initialState,
    reduce,
    isRevokedClose,
    needsTicker,
    createTicker,
    createHeartbeat,
    clockOffset,
    needsRevive,
  } = window.Companion;

  // ----------------------------------------------------------------- DOM --

  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined && text !== null) node.textContent = String(text);
    return node;
  };
  const clear = (node) => {
    while (node.firstChild) node.removeChild(node.firstChild);
    return node;
  };
  /** Append a run of `[className, text]` spans, skipping the empty ones. */
  const spans = (parent, ...pairs) => {
    for (const [className, text] of pairs) {
      if (text) parent.appendChild(el("span", className, text));
    }
    return parent;
  };
  const inlineNodes = (parent, parsed) => {
    for (const span of parsed) {
      if (span.bold) parent.appendChild(el("strong", null, span.text));
      else if (span.code) parent.appendChild(el("code", null, span.text));
      else parent.appendChild(document.createTextNode(span.text));
    }
  };
  /** Markdown tokens as real nodes; text only ever arrives via textContent. */
  const markdownNodes = (text) => {
    const wrap = el("div", "md");
    for (const block of parseMarkdown(text)) {
      if (block.type === "code") {
        wrap.appendChild(el("pre")).appendChild(el("code", null, block.text));
      } else if (block.items) {
        const list = wrap.appendChild(el(block.type));
        for (const item of block.items) inlineNodes(list.appendChild(el("li")), item);
      } else {
        inlineNodes(wrap.appendChild(el("p")), block.spans);
      }
    }
    return wrap;
  };
  function boot() {
    if (!document.getElementById("companion-root")) return;
    const $ = (id) => document.getElementById(id);
    let state = initialState();
    let socket = null;
    let attempt = 0;
    // Private browsing can refuse storage: the page works, it just forgets.
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
      render();
    };
    const dropToken = () => {
      store(TOKEN_KEY, null);
      if (socket) socket.close();
      socket = null;
      dispatch({ type: "unauthorized" });
    };
    /** A read allowed to be absent: 404 only means "nothing loaded there". */
    const read = async (path) => {
      const headers = state.token ? { Authorization: `Bearer ${state.token}` } : {};
      const response = await fetch(path, { headers });
      if (response.status === 401) dropToken();
      return response.ok ? await response.json() : null;
    };
    async function loadEverything() {
      const paths = [
        "/api/state",
        "/api/season",
        "/api/chat?screen=draft",
        "/api/chat?screen=season",
      ];
      const [draft, season, draftChat, seasonChat] = await Promise.all(paths.map(read));
      if (state.screen !== "app") return;
      dispatch({ type: "draft-updated", payload: draft });
      dispatch({ type: "season-updated", payload: season });
      dispatch({ type: "shared-chat", payload: draftChat });
      dispatch({ type: "shared-chat", payload: seasonChat });
      connect();
    }
    // ---- pairing -------------------------------------------------------
    $("pair-device").value = load(DEVICE_KEY) || deviceGuess(navigator.userAgent);
    state.hostName = load(HOST_KEY);
    $("pair-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const code = $("pair-code").value.trim();
      const device = $("pair-device").value.trim() || deviceGuess(navigator.userAgent);
      $("pair-submit").disabled = true;
      try {
        const response = await fetch("/api/pair", {
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
          const late = response.status === 429;
          const message = late ? "Too many tries — wait a minute." : "That code did not work.";
          dispatch({ type: "pair-error", message });
          return;
        }
        store(TOKEN_KEY, body.token);
        store(DEVICE_KEY, device);
        if (body.device_id) store(DEVICE_ID_KEY, body.device_id);
        if (body.host_name) store(HOST_KEY, body.host_name);
        dispatch({ type: "paired", token: body.token, hostName: body.host_name });
        await loadEverything();
      } catch {
        dispatch({ type: "pair-error", message: "The host did not answer." });
      } finally {
        $("pair-submit").disabled = false;
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
      const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${scheme}//${window.location.host}/api/events?token=${state.token}`);
      socket.onopen = () => {
        attempt = 0;
        dispatch({ type: "connection", status: "online" });
        heartbeat.start();
      };
      socket.onmessage = (event) => {
        let frame = null;
        try {
          frame = JSON.parse(event.data);
        } catch {
          return;
        }
        if (frame?.type === "pong") heartbeat.pong();
        else if (frame?.type === "hello") {
          heartbeat.pong();
          dispatch({
            type: "clock-offset",
            offset: clockOffset(frame.payload?.server_now_ms, Date.now()),
          });
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
        window.setTimeout(connect, backoffDelay(attempt));
        attempt += 1;
      };
    }

    // A phone that was asleep, in flight mode or off the Wi-Fi wakes with a
    // socket the operating system has already thrown away. Nothing errors, so
    // the page has to ask for itself the moment it is looked at again.
    const revive = () => {
      if (!state.token || state.screen !== "app") return;
      if (!needsRevive(socket)) return;
      if (socket) {
        // Detached first: its `onclose` would otherwise schedule a reconnect
        // of its own and the page would end up with two sockets.
        socket.onclose = null;
        socket.onmessage = null;
        socket.close();
      }
      heartbeat.stop();
      socket = null;
      attempt = 0;
      dispatch({ type: "connection", status: "reconnecting" });
      void loadEverything();
    };
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") revive();
    });
    window.addEventListener("pageshow", revive);
    window.addEventListener("online", revive);

    // ---- chat ----------------------------------------------------------
    /** Which thread the chat block shows: the Week tab is always the season. */
    const chatScreen = () => (state.tab === "week" || !state.draft ? "season" : "draft");
    $("chat-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      const screen = chatScreen();
      const text = $("chat-input").value.trim();
      if (!text) return;
      $("chat-input").value = "";
      dispatch({ type: "note", screen, message: null });
      const response = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${state.token}` },
        body: JSON.stringify({ screen, text }),
      });
      if (response.status === 401) dropToken();
      else if (!response.ok) {
        dispatch({ type: "note", screen, message: NOTES[response.status] ?? "That did not send." });
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
      if (!d) spans(strip, ["muted", "No draft is loaded on the host."]);
      else {
        // The host's clock, not this phone's: the deadline is the host's.
        const clock = formatClock(d.clock_deadline_ms, Date.now() + state.offset);
        spans(
          strip,
          ["headline", `Pick ${d.current_pick} · round ${d.current_round}`],
          [null, d.on_clock_name || `Slot ${d.on_clock_slot}`],
          ["mine", d.is_my_pick && "Your pick"],
          ["muted", clock && `${clock} left`],
        );
      }
      const recs = clear($("recs"));
      for (const rec of collapseAgreeing((view?.recommendations ?? []).slice(0, 5))) {
        const card = recs.appendChild(el("li", "card"));
        if (rec.mode) card.appendChild(el("div", "card-mode", rec.mode));
        const head = card.appendChild(el("div", "card-head"));
        spans(head, ["name", rec.name], [positionClass(rec.position), rec.position]);
        spans(head, ["muted", rec.team]);
        const survives = rec.survival_next;
        const facts = card.appendChild(el("div", "facts"));
        spans(
          facts,
          [null, `Tier ${rec.tier}`],
          [null, `ADP ${rec.adp === null ? "—" : rec.adp.toFixed(1)}`],
          [null, typeof survives === "number" && `Survives ${Math.round(survives * 100)}%`],
        );
        const list = card.appendChild(el("ul", "reasons"));
        for (const reason of rec.reasons ?? []) list.appendChild(el("li", null, reason));
      }
      const players = view?.my_roster?.players ?? [];
      const counts = {};
      for (const p of players) counts[p.position] = (counts[p.position] ?? 0) + 1;
      const drafted = Object.entries(counts)
        .map(([position, n]) => `${n} ${position}`)
        .join(" · ");
      const open = (view?.my_roster?.open_starters ?? [])
        .map(([slot, n]) => `${n} ${slot}`)
        .join(", ");
      spans(
        clear($("roster")),
        [null, drafted || "Nothing drafted yet."],
        ["muted", open && `Still to fill: ${open}`],
      );
      const failures =
        state.health?.consecutive_failures ?? view?.data_health?.poll_consecutive_failures ?? null;
      const sync = failures ? `${failures} failed syncs` : failures === 0 ? "sync healthy" : "";
      const size = view ? `${view.data_health.board_size} players on the board` : "";
      $("health").textContent = [size, sync].filter(Boolean).join(" · ");
    }

    function renderPicks() {
      const list = clear($("picks"));
      const view = state.draft;
      const picks = [...(view?.recent_picks ?? [])].sort((a, b) => b.pick_no - a.pick_no);
      const mySlot = view?.draft?.my_slot ?? null;
      for (const pick of picks.slice(0, 25)) {
        const row = list.appendChild(el("li", pick.slot === mySlot ? "row is-mine" : "row"));
        spans(row, ["pick-no", `${pick.round}.${pick.pick_no}`], ["name", pick.name]);
        spans(row, [positionClass(pick.position), pick.position]);
        spans(row, ["muted", pick.slot_name || `Slot ${pick.slot}`]);
      }
      if (!picks.length) list.appendChild(el("li", "muted", "No picks yet."));
    }

    function renderChat() {
      const screen = chatScreen();
      $(state.tab === "week" ? "tab-week" : "tab-chat").appendChild($("chat-block"));
      const thread = state.chat[screen];
      const list = clear($("chat-list"));
      const entries = thread?.entries ?? [];
      const now = Date.now();
      for (const entry of entries) {
        const item = list.appendChild(el("li", `entry ${entry.role}`));
        const who = entry.device?.name ?? "Someone";
        spans(
          item.appendChild(el("div", "entry-meta")),
          [null, entry.role === "assistant" ? `Answer for ${who}` : `${who} asked`],
          ["kind", entry.device?.kind],
          [null, relativeTime(entry.at_ms, now)],
          [null, formatCost(entry.cost_usd)],
        );
        if (entry.error) item.appendChild(el("p", "error", entry.error));
        else if (entry.role === "assistant") item.appendChild(markdownNodes(entry.text));
        else item.appendChild(el("p", null, entry.text));
      }
      if (!entries.length) list.appendChild(el("li", "muted", "Nothing asked yet."));
      $("chat-note").hidden = !state.note[screen];
      $("chat-note").textContent = state.note[screen] ?? "";
      const busy = Boolean(thread?.busy);
      $("chat-input").disabled = busy;
      $("chat-send").disabled = busy;
      $("chat-send").textContent = busy ? "Answering…" : "Send";
    }

    function renderWeek() {
      const view = state.season;
      const header = clear($("week-header"));
      if (!view) return;
      const head = view.header ?? {};
      const live = view.live?.totals;
      const projected =
        typeof head.my_projected === "number" &&
        `Projected ${head.my_projected.toFixed(1)} – ${head.opp_projected.toFixed(1)}`;
      spans(
        header,
        ["headline", `Week ${view.week}`],
        [null, `${view.matchup?.my_name ?? "You"} vs ${head.opponent_name ?? "—"}`],
        ["mine", live && `${live.my_live_points.toFixed(1)} – ${live.opp_live_points.toFixed(1)}`],
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
        renderWeek();
        renderChat();
      }
    }

    const saved = load(TOKEN_KEY);
    if (saved) {
      state = { ...state, token: saved, screen: "app" };
      void loadEverything();
    }
    render();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
  else boot();
})();
