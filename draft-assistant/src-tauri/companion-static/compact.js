/* Draft Assistant companion page, the compact switch.
   One tap trims the page for a small screen or a long draft: tighter cards,
   one reason per recommendation, no suggestion chips. The choice is a class
   on the root, so the trimming is all in extras.css, and it is remembered on
   this device. Loaded after helpers.js and before app.js; it adds to
   `window.Companion`. */
(() => {
  "use strict";
  const COMPACT_KEY = "da.companion.compact";
  const createCompact = (doc, win) => {
    const root = doc.getElementById("companion-root");
    const toggle = doc.getElementById("compact-toggle");
    let on = false;
    try {
      on = win.localStorage.getItem(COMPACT_KEY) === "on";
    } catch {
      on = false;
    }
    const paint = () => {
      root?.classList.toggle("compact", on);
      if (!toggle) return;
      toggle.textContent = on ? "Compact on" : "Compact off";
      toggle.setAttribute("aria-pressed", String(on));
      toggle.title = on ? "Back to the roomier layout" : "Tighter layout, fewer words";
    };
    toggle?.addEventListener("click", () => {
      on = !on;
      try {
        win.localStorage.setItem(COMPACT_KEY, on ? "on" : "off");
      } catch {
        /* Session-only choice. */
      }
      paint();
    });
    paint();
    return { enabled: () => on };
  };
  window.Companion = { ...window.Companion, COMPACT_KEY, createCompact };
})();
