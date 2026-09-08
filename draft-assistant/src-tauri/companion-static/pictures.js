/* Draft Assistant companion page, the faces.
   Player headshots and team logos on the phone's rows, the same reading as
   the desktop's Headshot component: a team mark straight from Sleeper's CDN
   goes up at once, and the player's photo replaces it when the host has
   fetched one. Photos come through the host (`/api/headshot/<id>`) with the
   pairing token, arrive as bytes and are read into a data URL, since the
   page's CSP lets an <img> show only itself, data: and sleepercdn.com.
   Each face is asked for once per page; a "no photo" answer is remembered,
   a failed request is not. Loaded after models.js and before app.js; it adds
   to `window.Companion`. */
(() => {
  "use strict";
  /** Pixels on a side: the row height the phone's cards were drawn for. */
  const SIZE = 28;
  const LOGO_BASE = "https://sleepercdn.com/images/team_logos/nfl/";

  /** Real players have numeric ids; a defence's id is its team code. */
  const isPlayerId = (id) => typeof id === "string" && /^\d+$/.test(id);

  /** Sleeper's logo for a team code, or null with no team to name. */
  const teamLogo = (team) =>
    typeof team === "string" && team ? `${LOGO_BASE}${team.toLowerCase()}.png` : null;

  /** The team whose mark stands in for the face. A defence carries no
   *  separate team, so its own id ("JAX") is the mark. */
  const markTeam = (player) => {
    const team = player.team;
    if (typeof team === "string" && team) return team;
    const id = player.player_id;
    return typeof id === "string" && id && !isPlayerId(id) ? id : null;
  };

  /** Read image bytes into a data URL through the window's FileReader.
   *  A window without one (an old test window, a stripped webview) gets
   *  null, which the caller draws as "no photo" rather than a broken image.
   *  The document's own window is the same object on a phone and is the
   *  fallback for a harness window that carries no reader of its own. */
  const readDataUrl = (win, blob) => {
    const Reader = win.FileReader ?? document.defaultView?.FileReader;
    if (typeof Reader !== "function") return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const reader = new Reader();
      reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
      reader.onerror = () => reject(reader.error ?? new Error("headshot read failed"));
      reader.readAsDataURL(blob);
    });
  };

  /** Whatever `fetch` the window has; the page's global one on a phone is
   *  the same function, and the test harness supplies only the global. */
  const pickFetch = (win) => {
    if (typeof win.fetch === "function") return (url, init) => win.fetch(url, init);
    if (typeof fetch === "function") return (url, init) => fetch(url, init);
    return null;
  };

  /** The page's picture loader. `getToken` is read at request time, so a
   *  page that pairs after boot starts fetching without being rebuilt. */
  const createPictures = (win, getToken) => {
    const fetchFn = pickFetch(win);
    /** Player id to the promise of its data URL, or null for "no photo".
     *  One entry per id for the life of the page; see `headshot`. */
    const faces = new Map();

    /** The data URL for a player's photo, or null when there is none.
     *
     *  A resolved null is an answer (Sleeper has no picture) and is kept, so
     *  every repaint does not ask again. A rejection is not an answer: the
     *  request failed (offline, a timeout, the host mid-restart), and keeping
     *  it would blank that one face until the page reloads. So the entry is
     *  dropped on failure and the next row that wants this player asks
     *  afresh. No retry timer: a logo that becomes a photo on the next
     *  render is the whole of the recovery. The desktop's avatars.ts makes
     *  the same choice for the same reason. */
    const headshot = (id) => {
      const known = faces.get(id);
      if (known) return known;
      let token = null;
      try {
        token = getToken();
      } catch {
        token = null;
      }
      if (!token || !fetchFn) return Promise.resolve(null);
      const pending = new Promise((resolve) =>
        resolve(
          fetchFn(`/api/headshot/${encodeURIComponent(id)}`, {
            headers: { Authorization: `Bearer ${token}` },
          }),
        ),
      )
        .then((response) =>
          response && response.ok ? response.blob().then((blob) => readDataUrl(win, blob)) : null,
        )
        .catch(() => {
          // Only forget this entry: a later ask that already replaced it
          // must not have its answer torn out by this stale failure.
          if (faces.get(id) === pending) faces.delete(id);
          return null;
        });
      faces.set(id, pending);
      return pending;
    };

    /** The round picture slot for a row: `<span class="avatar">` around an
     *  <img>. The logo is set at once and the photo swaps in when it lands;
     *  a photo that fails to draw goes back to the logo, and a logo that
     *  fails hides the image. A player with neither gets the empty slot, so
     *  the row keeps its shape. Never throws: a face is never worth a row. */
    const avatar = (player) => {
      const span = document.createElement("span");
      span.className = "avatar";
      try {
        const subject = player && typeof player === "object" ? player : {};
        const logo = teamLogo(markTeam(subject));
        const wanted = isPlayerId(subject.player_id) ? subject.player_id : null;
        if (!logo && !wanted) {
          span.classList.add("avatar-blank");
          return span;
        }
        const img = document.createElement("img");
        img.setAttribute("alt", "");
        img.setAttribute("width", String(SIZE));
        img.setAttribute("height", String(SIZE));
        img.setAttribute("loading", "lazy");
        img.setAttribute("decoding", "async");
        span.appendChild(img);
        let face = null;
        const show = (src) => {
          if (src) {
            img.setAttribute("src", src);
            span.classList.remove("avatar-blank");
          } else {
            img.removeAttribute("src");
            span.classList.add("avatar-blank");
          }
        };
        img.addEventListener("error", () => {
          if (face && img.getAttribute("src") === face) {
            face = null;
            show(logo);
          } else show(null);
        });
        show(logo);
        if (wanted) {
          headshot(wanted).then((url) => {
            if (!url) return;
            face = url;
            show(url);
          });
        }
      } catch {
        // Whatever went wrong, the row still gets its slot.
      }
      return span;
    };

    return { avatar };
  };

  window.Companion = { ...window.Companion, createPictures };
})();
