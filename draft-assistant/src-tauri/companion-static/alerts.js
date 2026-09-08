/* Draft Assistant companion page, the nudge.
   A phone in a hand or on a table has to say out loud when the pick is
   yours and when your clock is nearly gone: a short tone, a buzz where the
   phone can (Android; iPhones ignore `vibrate`), a toast that jumps to the
   Now tab, and the tab title. The decision is pure and remembers which
   pick it already spoke for, so a page that repaints every second nudges
   once per pick and once per low clock, never on every tick.
   Sound needs a user gesture on iPhone before the page may make a noise,
   so the Alerts button and the first tap anywhere arm it. Loaded after
   helpers.js and before app.js; it adds to `window.Companion`. */
(() => {
  "use strict";
  const ALERTS_KEY = "da.companion.alerts";
  /** Seconds left on your own clock at which the second nudge fires. */
  const LOW_SECS = 15;
  /** How long the toast stays up, in milliseconds. */
  const TOAST_MS = 6000;
  const VIBRATE = { "your-turn": [200, 100, 200, 100, 400], "low-time": [120, 60, 120] };

  /** Same reading as the clock strip: a pre-draft with only keepers is waiting. */
  const waiting = (d) =>
    d.status === "pre_draft" && (d.total_picks_made ?? 0) <= (d.keeper_picks?.length ?? 0);

  /** The nudge this draft snapshot earns, given what was already nudged.
   *  `memory` is `{ pick, low }`: the pick number that got a your-turn, and
   *  the pick number that got a low-time. Returns the new memory and the
   *  alert, if any. Pure. */
  const nextAlert = (memory, draft, nowMs) => {
    const d = draft?.draft;
    const kept = memory ?? { pick: null, low: null };
    if (!d || d.status === "complete" || d.paused || d.status === "paused" || d.is_auction)
      return { memory: kept, alert: null };
    if (waiting(d) || !d.is_my_pick || !Number.isInteger(d.current_pick))
      return { memory: kept, alert: null };
    const pick = d.current_pick;
    if (kept.pick !== pick) {
      return { memory: { pick, low: null }, alert: { kind: "your-turn", pick } };
    }
    const left =
      typeof d.clock_deadline_ms === "number"
        ? Math.ceil((d.clock_deadline_ms - nowMs) / 1000)
        : null;
    if (left !== null && left > 0 && left <= LOW_SECS && kept.low !== pick) {
      return { memory: { pick, low: pick }, alert: { kind: "low-time", pick } };
    }
    return { memory: kept, alert: null };
  };

  const TOAST = {
    "your-turn": "Your pick is up",
    "low-time": `Under ${LOW_SECS} seconds on your pick`,
  };

  /** A two-note chirp from an oscillator; nothing to download. `ctx` is an
   *  AudioContext that has been resumed by a gesture, or the call is a no-op. */
  const chirp = (ctx, kind) => {
    if (!ctx || ctx.state !== "running") return false;
    const notes = kind === "low-time" ? [880, 880, 880] : [660, 880];
    let at = ctx.currentTime;
    for (const hz of notes) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = hz;
      gain.gain.setValueAtTime(0.0001, at);
      gain.gain.exponentialRampToValueAtTime(0.4, at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.18);
      osc.connect(gain).connect(ctx.destination);
      osc.start(at);
      osc.stop(at + 0.2);
      at += 0.22;
    }
    return true;
  };

  /** The alerts as wired to a page: a toggle button, a toast, storage, and
   *  `observe(state, nowMs)` for app.js to call after every render. */
  const createAlerts = (doc, win, { goNow } = {}) => {
    const toggle = doc.getElementById("alerts-toggle");
    const toast = doc.getElementById("alert-toast");
    let memory = null;
    let ctx = null;
    let hideTimer = null;
    const load = () => {
      try {
        return win.localStorage.getItem(ALERTS_KEY);
      } catch {
        return null;
      }
    };
    let on = load() !== "off";
    const store = () => {
      try {
        win.localStorage.setItem(ALERTS_KEY, on ? "on" : "off");
      } catch {
        return;
      }
    };
    /** Make (or wake) the audio context. Only worth calling from a gesture. */
    const arm = () => {
      const Audio = win.AudioContext || win.webkitAudioContext;
      if (typeof Audio !== "function") return;
      try {
        if (!ctx) ctx = new Audio();
        if (ctx.state === "suspended") void ctx.resume().catch(() => {});
      } catch {
        ctx = null;
      }
    };
    const paint = () => {
      if (!toggle) return;
      toggle.textContent = on ? "Alerts on" : "Alerts off";
      toggle.setAttribute("aria-pressed", String(on));
      toggle.title = on
        ? "A tone and a buzz when your pick is up. Sound follows the ring/silent switch."
        : "Turn on a tone and a buzz for your pick";
    };
    const showToast = (kind) => {
      if (!toast) return;
      toast.textContent = TOAST[kind];
      toast.dataset.kind = kind;
      toast.hidden = false;
      if (hideTimer !== null) win.clearTimeout(hideTimer);
      hideTimer = win.setTimeout(() => {
        toast.hidden = true;
        hideTimer = null;
      }, TOAST_MS);
    };
    const fire = (kind) => {
      showToast(kind);
      if (!on) return;
      if (typeof win.navigator?.vibrate === "function") {
        try {
          win.navigator.vibrate(VIBRATE[kind]);
        } catch {
          /* A browser that lists it and refuses it. */
        }
      }
      chirp(ctx, kind);
    };
    toggle?.addEventListener("click", () => {
      on = !on;
      store();
      paint();
      if (on) {
        arm();
        // The tap is the gesture; a short chirp is the proof it worked.
        chirp(ctx, "your-turn");
      }
    });
    toast?.addEventListener("click", () => {
      toast.hidden = true;
      goNow?.();
    });
    // Any first touch on the page is a gesture too, so a phone that had
    // alerts on from last time is armed without finding the button.
    const armOnce = () => {
      if (on) arm();
      doc.removeEventListener("pointerdown", armOnce);
    };
    doc.addEventListener("pointerdown", armOnce);
    paint();
    return {
      observe: (state, nowMs) => {
        if (!state || state.screen !== "app") return;
        const next = nextAlert(memory, state.draft, nowMs);
        memory = next.memory;
        if (next.alert) fire(next.alert.kind);
      },
      enabled: () => on,
      armed: () => Boolean(ctx && ctx.state === "running"),
    };
  };

  window.Companion = {
    ...window.Companion,
    ALERTS_KEY,
    LOW_SECS,
    TOAST_MS,
    nextAlert,
    chirp,
    createAlerts,
  };
})();
