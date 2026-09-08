/* Independent of the app's helpers: explain failed or blocked startup scripts.
   No inline script is needed, and no connection credentials enter diagnostics. */
(() => {
  "use strict";
  const failed = () => {
    const form = document.getElementById("pair-form");
    if (!form || form.dataset.ready === "true") return;
    const status = document.getElementById("boot-status");
    if (!status) return;
    status.hidden = false;
    const reload = document.getElementById("boot-reload");
    if (reload) {
      reload.hidden = false;
      // A script error and the timer can both land here; one listener is enough.
      if (reload.dataset.bound !== "true") {
        reload.dataset.bound = "true";
        reload.addEventListener("click", () => window.location.reload());
      }
    }
    status.textContent =
      "Connection controls could not load. Reload this page, or open this address directly in Safari or Chrome with JavaScript enabled. Keep Tailscale connected when using the remote address.";
  };
  window.addEventListener(
    "error",
    (event) => {
      if (event.target?.tagName === "SCRIPT" || event.message) failed();
    },
    true,
  );
  window.setTimeout(failed, 10000);
})();
