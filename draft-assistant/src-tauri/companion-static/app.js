/* Draft Assistant companion page. Plain script, no build step: it is
   include_str!'d into the desktop binary and served under a CSP that allows
   nothing inline, so every behaviour lives in this file.
   The pure parts — relative time, position colours, the markdown parser and
   the state reducer — hang off `window.Companion` so a unit test can drive
   them without a DOM. */
(() => {
  "use strict";
  const TOKEN_KEY = "da.companion.token";
  const DEVICE_KEY = "da.companion.device";
  const HOST_KEY = "da.companion.host";
  const REVOKED = "Pairing was revoked — ask the host for the new code";
  const TABS = ["now", "picks", "chat", "week"];
  const LIVE = ["draft-updated", "season-updated", "shared-chat", "poll-health"];
  const NOTES = { 409: "The host is still answering.", 429: "Too many questions — slow down." };

  // ---------------------------------------------------------------- pure --

  /** A first guess at what to call this phone, from the user agent. */
  const deviceGuess = (ua = "") => {
    if (/iPad/i.test(ua)) return "iPad";
    if (/iPhone|iPod/i.test(ua)) return "iPhone";
    if (/Android/i.test(ua)) return "Android phone";
    return "Phone";
  };
  /** "just now" / "4m ago" / "3h ago" / "2d ago". Never a bare timestamp. */
  const relativeTime = (atMs, nowMs) => {
    const seconds = Math.round((nowMs - atMs) / 1000);
    if (!isFinite(seconds) || seconds < 45) return "just now";
    if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`;
    return `${Math.round(seconds / 86400)}d ago`;
  };
  /** Class list for a position pill; an unknown position stays neutral. */
  const positionClass = (position) => {
    const key = String(position ?? "").toLowerCase();
    return ["qb", "rb", "wr", "te", "k", "def"].includes(key) ? `pos pos-${key}` : "pos";
  };
  /** Reconnect delay: one second, doubling to a thirty second ceiling. */
  const backoffDelay = (attempt) => Math.min(30000, 1000 * 2 ** Math.max(0, attempt));

  const formatCost = (usd) =>
    typeof usd === "number" && isFinite(usd) ? `$${usd.toFixed(usd < 0.01 ? 4 : 2)}` : null;
  /** "0:45" left on the pick clock, or null when no clock is running. */
  const formatClock = (deadlineMs, nowMs) => {
    if (typeof deadlineMs !== "number") return null;
    const left = Math.max(0, Math.round((deadlineMs - nowMs) / 1000));
    return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
  };
  /** Inline spans: **bold** and `code`; everything else is literal text. */
  const parseInline = (text) => {
    const spans = [];
    const pattern = /\*\*([^*]+)\*\*|`([^`]+)`/g;
    let last = 0;
    for (let m = pattern.exec(text); m; m = pattern.exec(text)) {
      if (m.index > last) spans.push({ text: text.slice(last, m.index) });
      spans.push(m[1] === undefined ? { text: m[2], code: true } : { text: m[1], bold: true });
      last = m.index + m[0].length;
    }
    if (last < text.length) spans.push({ text: text.slice(last) });
    return spans;
  };
  const BULLET = /^\s*[-*]\s+(.*)$/;
  const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
  const FENCE = /^\s*```/;
  /** A deliberately small markdown subset — paragraphs, lists, fenced code,
   *  bold and inline code — as tokens. Nothing here produces HTML: the
   *  renderer makes text nodes, so `<script>` renders as the word. */
  const parseMarkdown = (text) => {
    const lines = String(text ?? "").split(/\r?\n/);
    const blocks = [];
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      if (FENCE.test(line)) {
        const code = [];
        for (i += 1; i < lines.length && !FENCE.test(lines[i]); i += 1) code.push(lines[i]);
        i += 1;
        blocks.push({ type: "code", text: code.join("\n") });
      } else if (BULLET.test(line) || NUMBERED.test(line)) {
        const rule = BULLET.test(line) ? BULLET : NUMBERED;
        const items = [];
        for (let m = rule.exec(lines[i]); m; m = i < lines.length ? rule.exec(lines[i]) : null) {
          items.push(parseInline(m[1]));
          i += 1;
        }
        blocks.push({ type: rule === BULLET ? "ul" : "ol", items });
      } else if (line.trim() === "") {
        i += 1;
      } else {
        const paragraph = [];
        while (i < lines.length && lines[i].trim() !== "") {
          if (FENCE.test(lines[i]) || BULLET.test(lines[i]) || NUMBERED.test(lines[i])) break;
          paragraph.push(lines[i].replace(/^#+\s*/, ""));
          i += 1;
        }
        blocks.push({ type: "p", spans: parseInline(paragraph.join(" ")) });
      }
    }
    return blocks;
  };
  const initialState = () => ({
    screen: "pair",
    tab: "now",
    token: null,
    hostName: null,
    pairError: null,
    draft: null,
    season: null,
    chat: {},
    note: {},
    health: null,
    connection: "online",
  });
  /** The whole client state machine, one pure function per action. */
  const ACTIONS = {
    paired: (s, a) => ({ ...s, screen: "app", token: a.token, hostName: a.hostName || s.hostName }),
    "pair-error": (s, a) => ({ ...s, pairError: a.message }),
    unauthorized: (s) => ({ ...initialState(), hostName: s.hostName, pairError: REVOKED }),
    "draft-updated": (s, a) => ({ ...s, draft: a.payload ?? null }),
    "season-updated": (s, a) => ({
      ...s,
      season: a.payload ?? null,
      tab: !a.payload && s.tab === "week" ? "now" : s.tab,
    }),
    "shared-chat": (s, a) =>
      a.payload?.screen ? { ...s, chat: { ...s.chat, [a.payload.screen]: a.payload } } : s,
    "poll-health": (s, a) => ({ ...s, health: a.payload ?? null }),
    note: (s, a) => ({ ...s, note: { ...s.note, [a.screen]: a.message } }),
    tab: (s, a) =>
      TABS.includes(a.tab) && (a.tab !== "week" || s.season) ? { ...s, tab: a.tab } : s,
    connection: (s, a) => ({ ...s, connection: a.status }),
  };
  const reduce = (state, action) => ACTIONS[action.type]?.(state, action) ?? state;
  window.Companion = {
    REVOKED,
    deviceGuess,
    relativeTime,
    positionClass,
    backoffDelay,
    formatCost,
    formatClock,
    parseMarkdown,
    initialState,
    reduce,
  };

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
    let pingTimer = null;

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
          body: JSON.stringify({ code, device_name: device, kind: "phone" }),
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
    function connect() {
      if (!state.token) return;
      const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${scheme}//${window.location.host}/api/events?token=${state.token}`);
      socket.onopen = () => {
        attempt = 0;
        dispatch({ type: "connection", status: "online" });
        pingTimer = window.setInterval(() => {
          if (socket && socket.readyState === 1) socket.send(JSON.stringify({ type: "ping" }));
        }, 25000);
      };
      socket.onmessage = (event) => {
        let frame = null;
        try {
          frame = JSON.parse(event.data);
        } catch {
          return;
        }
        if (frame?.type === "revoked") dropToken();
        else if (LIVE.includes(frame?.type)) dispatch({ type: frame.type, payload: frame.payload });
      };
      socket.onclose = () => {
        window.clearInterval(pingTimer);
        if (!state.token) return;
        dispatch({ type: "connection", status: "reconnecting" });
        window.setTimeout(connect, backoffDelay(attempt));
        attempt += 1;
      };
    }

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
    function renderNow() {
      const view = state.draft;
      const strip = clear($("clock-strip"));
      const d = view?.draft;
      if (!d) spans(strip, ["muted", "No draft is loaded on the host."]);
      else {
        const clock = formatClock(d.clock_deadline_ms, Date.now());
        spans(
          strip,
          ["headline", `Pick ${d.current_pick} · round ${d.current_round}`],
          [null, d.on_clock_name || `Slot ${d.on_clock_slot}`],
          ["mine", d.is_my_pick && "Your pick"],
          ["muted", clock && `${clock} left`],
        );
      }
      const recs = clear($("recs"));
      for (const rec of (view?.recommendations ?? []).slice(0, 5)) {
        const card = recs.appendChild(el("li", "card"));
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
