/* Draft Assistant companion page, coming back where it was.
   A reload, a Safari that threw the tab away, an installed app relaunched:
   the page should open on the tab it was on, scrolled to where it was, not
   on Now at the top. The tab is remembered on the device; how far down each
   tab was is remembered for the session, which is what a reload keeps and a
   fresh open does not. Loaded before app.js; it adds to `window.Companion`. */
(() => {
  "use strict";
  const TAB_KEY = "da.companion.tab";
  const SCROLL_PREFIX = "da.companion.scroll.";
  /** How often, at most, a scroll position is written while the thumb moves. */
  const SAVE_EVERY_MS = 250;

  const createRestore = (win, tabs) => {
    const local = () => win.localStorage;
    const session = () => win.sessionStorage;
    const restored = new Set();
    let timer = null;
    const read = (store, key) => {
      try {
        return store().getItem(key);
      } catch {
        return null;
      }
    };
    const write = (store, key, value) => {
      try {
        store().setItem(key, value);
      } catch {
        /* Private browsing: nothing survives, which is fine. */
      }
    };
    return {
      /** The tab to open on, if a remembered one is still a tab. */
      tab: () => {
        const saved = read(local, TAB_KEY);
        return tabs.includes(saved) ? saved : null;
      },
      rememberTab: (tab) => write(local, TAB_KEY, tab),
      /** Called on every scroll; writes at most a few times a second. */
      saveScroll: (tab) => {
        if (timer !== null) return;
        timer = win.setTimeout(() => {
          timer = null;
          write(session, SCROLL_PREFIX + tab, String(Math.round(win.scrollY || 0)));
        }, SAVE_EVERY_MS);
      },
      /** Once per tab per page load, and only when there is content to be
       *  scrolled into: called after the tab has painted real data. */
      restoreScroll: (tab) => {
        if (restored.has(tab)) return false;
        restored.add(tab);
        const saved = Number(read(session, SCROLL_PREFIX + tab));
        if (!(saved > 0)) return false;
        win.scrollTo?.(0, saved);
        return true;
      },
    };
  };

  window.Companion = { ...window.Companion, TAB_KEY, SCROLL_PREFIX, createRestore };
})();
