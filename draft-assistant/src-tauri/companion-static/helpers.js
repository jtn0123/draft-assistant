/* Draft Assistant companion page — the pure half. Constants, formatting,
   the markdown parser and the state reducer: everything that needs no DOM,
   published on `window.Companion` so `app.js` and a unit test can share it.
   Loaded before app.js; no build step, nothing inline (the CSP forbids it). */
(() => {
  "use strict";
  const TOKEN_KEY = "da.companion.token";
  const DEVICE_KEY = "da.companion.device";
  const DEVICE_ID_KEY = "da.companion.device-id";
  const HOST_KEY = "da.companion.host";
  const REVOKED = "The host restarted or revoked this device. Pair again.";
  const TABS = ["now", "picks", "chat", "week"];
  const LIVE = [
    "draft-updated",
    "season-updated",
    "shared-chat",
    "poll-health",
    "season-poll-health",
  ];
  const NOTES = { 409: "The host is still answering.", 429: "Too many questions. Slow down." };
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
  const modeLabel = (mode) =>
    typeof mode === "string" && mode ? mode[0].toUpperCase() + mode.slice(1) : "";
  /** Three cards for one player is one card; modes that agree share a label. */
  const collapseAgreeing = (recs) => {
    const out = [];
    for (const rec of recs) {
      const same = out.find((r) => r.player_id === rec.player_id);
      if (same) same.mode = `${same.mode} · ${modeLabel(rec.mode)}`;
      else out.push({ ...rec, mode: modeLabel(rec.mode) });
    }
    return out.map((r) => (r.mode.includes(" · ") ? { ...r, mode: `${r.mode} agree` } : r));
  };
  /** Class list for a position pill; an unknown position stays neutral. */
  const positionClass = (position) => {
    const key = String(position ?? "").toLowerCase();
    return ["qb", "rb", "wr", "te", "k", "def"].includes(key) ? `pos pos-${key}` : "pos";
  };
  /** How long one request to the host may take before the page gives up on
   *  it. A host that took the connection and never answered left a question
   *  hanging with no note and the Send button doing nothing. */
  const HOST_TIMEOUT_MS = 10000;
  /** The name on the error `timedFetch` rejects with when the deadline ran out. */
  const HOST_TIMEOUT = "HostTimeout";
  /** `fetch` with a deadline: rejects with a `HOST_TIMEOUT` error after `ms`
   *  of silence, and with the network's own error for anything else. The
   *  timer and the controller come off `win`, so a page booted over fake
   *  timers can wind the clock; a window with no `AbortController` (there is
   *  none in the test's bare context) gets the plain request. */
  const timedFetch = (win, fetchFn, url, init = {}, ms = HOST_TIMEOUT_MS) => {
    const Controller = win?.AbortController;
    if (typeof Controller !== "function" || typeof win.setTimeout !== "function") {
      return fetchFn(url, init);
    }
    const controller = new Controller();
    const timer = win.setTimeout(() => controller.abort(), ms);
    return fetchFn(url, { ...init, signal: controller.signal }).then(
      (response) => {
        win.clearTimeout(timer);
        return response;
      },
      (error) => {
        win.clearTimeout(timer);
        if (!controller.signal.aborted) throw error;
        const late = new Error("The host did not answer in time.");
        late.name = HOST_TIMEOUT;
        throw late;
      },
    );
  };
  /** Reconnect delay: one second, doubling to a thirty second ceiling. */
  const backoffDelay = (attempt) => Math.min(30000, 1000 * 2 ** Math.max(0, attempt));
  const formatCost = (usd) =>
    typeof usd === "number" && isFinite(usd) && usd > 0
      ? `$${usd.toFixed(usd < 0.01 ? 4 : 2)}`
      : null;
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
    seasonHealth: null,
    connection: "online",
    /** How far the host's clock is ahead of this one, in milliseconds. */
    offset: 0,
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
    "season-poll-health": (s, a) => ({ ...s, seasonHealth: a.payload ?? null }),
    note: (s, a) => ({ ...s, note: { ...s.note, [a.screen]: a.message } }),
    tab: (s, a) =>
      TABS.includes(a.tab) && (a.tab !== "week" || s.season) ? { ...s, tab: a.tab } : s,
    connection: (s, a) => ({ ...s, connection: a.status }),
    "clock-offset": (s, a) => ({ ...s, offset: a.offset }),
  };
  const reduce = (state, action) => ACTIONS[action.type]?.(state, action) ?? state;
  // ---------------------------------------------------- node builders --
  // Nothing here reads the page; each makes nodes for app.js to place.
  // Here rather than in app.js only to keep that file under the size cap.

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
  /** The chat thread's list items, rebuilt whole: who asked or was answered,
   *  when (stamped on the node so a tick can move it), what it cost, and the
   *  answer as the markdown subset above. */
  const buildChatList = (list, entries, now) => {
    clear(list);
    for (const entry of entries) {
      const item = list.appendChild(el("li", `entry ${entry.role}`));
      const who = entry.device?.name ?? "Someone";
      const meta = item.appendChild(el("div", "entry-meta"));
      spans(
        meta,
        [null, entry.role === "assistant" ? `Answer for ${who}` : `${who} asked`],
        ["kind", entry.device?.kind],
      );
      const when = meta.appendChild(el("span", null, relativeTime(entry.at_ms, now)));
      when.dataset.at = String(entry.at_ms);
      spans(meta, [null, formatCost(entry.cost_usd)]);
      if (entry.error) item.appendChild(el("p", "error", entry.error));
      else if (entry.role === "assistant") item.appendChild(markdownNodes(entry.text));
      else item.appendChild(el("p", null, entry.text));
    }
    if (!entries.length) list.appendChild(el("li", "muted", "Nothing asked yet."));
  };
  window.Companion = {
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
    modeLabel,
    collapseAgreeing,
    backoffDelay,
    HOST_TIMEOUT_MS,
    HOST_TIMEOUT,
    timedFetch,
    formatCost,
    formatClock,
    parseMarkdown,
    initialState,
    reduce,
    el,
    clear,
    spans,
    markdownNodes,
    buildChatList,
  };
})();
