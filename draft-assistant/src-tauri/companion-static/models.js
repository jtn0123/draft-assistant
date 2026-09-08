/* Device-local model choice. The host remains responsible for credentials. */
(() => {
  "use strict";
  const KEY = "da.companion.model-choice";
  window.Companion.createMobileModels = (doc, host) => {
    const toggle = doc.getElementById("model-toggle");
    const options = doc.getElementById("model-options");
    const panel = doc.getElementById("model-panel");
    const effort = doc.getElementById("model-effort");
    const note = doc.getElementById("model-note");
    let chosen = { model: "Opus 5", effort: "High" };
    let catalog = [];
    let busy = false;
    try {
      const saved = JSON.parse(host.localStorage.getItem(KEY));
      if (typeof saved?.model === "string" && typeof saved?.effort === "string") chosen = saved;
    } catch {
      /* Storage can be unavailable in private browsing. */
    }
    const remember = () => {
      try {
        host.localStorage.setItem(KEY, JSON.stringify(chosen));
      } catch {
        /* Session-only choice. */
      }
    };
    const current = () => catalog.find((item) => item.model === chosen.model);
    const collapse = () => {
      panel.hidden = true;
      toggle.setAttribute("aria-expanded", "false");
    };
    const paint = () => {
      const item = current();
      const label = catalog.length ? chosen.model : "Host default";
      // One short line in the chat heading; the choices open under it on tap.
      toggle.textContent = catalog.length ? `${label} · ${chosen.effort} ▾` : label;
      toggle.setAttribute("aria-label", `Model: ${label}`);
      if (!catalog.length) collapse();
      toggle.disabled = busy || !catalog.length;
      effort.replaceChildren();
      for (const level of item?.efforts ?? [""]) {
        const option = doc.createElement("option");
        option.value = level;
        option.textContent = level === "" ? "Unknown" : level === "xhigh" ? "X-High" : level;
        effort.appendChild(option);
      }
      effort.value = catalog.length ? chosen.effort : "";
      effort.disabled = busy || !catalog.length;
      doc.getElementById("chat-send").disabled = busy || (catalog.length > 0 && !item?.available);
      options.replaceChildren();
      for (const model of catalog) {
        const button = doc.createElement("button");
        button.type = "button";
        button.textContent = model.model;
        if (typeof model.note === "string" && model.note) {
          const hint = doc.createElement("span");
          hint.className = "model-hint";
          hint.textContent = model.note;
          button.appendChild(hint);
        }
        button.setAttribute(
          "aria-label",
          model.note ? `${model.model}, ${model.note}` : model.model,
        );
        button.dataset.model = model.model;
        button.disabled = busy || !model.available;
        button.setAttribute("aria-pressed", String(model.model === chosen.model));
        button.title = model.available
          ? "Use this model for your next question"
          : "Unavailable on the host";
        button.addEventListener("click", () => {
          chosen.model = model.model;
          if (!model.efforts.includes(chosen.effort))
            chosen.effort = model.efforts.includes("High") ? "High" : model.efforts[0];
          remember();
          collapse();
          paint();
          toggle.focus();
        });
        options.appendChild(button);
      }
      note.textContent = !catalog.length
        ? "Host model choices unavailable. The host will use its default."
        : !item?.available
          ? "This model is unavailable on the host. Choose another model."
          : "";
      note.hidden = !note.textContent;
    };
    toggle.addEventListener("click", () => {
      panel.hidden = !panel.hidden;
      toggle.setAttribute("aria-expanded", String(!panel.hidden));
    });
    options.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        collapse();
        toggle.focus();
      }
    });
    effort.addEventListener("change", () => {
      chosen.effort = effort.value;
      remember();
      paint();
    });
    paint();
    return {
      configure(data) {
        catalog = Array.isArray(data?.models)
          ? data.models.filter(
              (item) =>
                typeof item.model === "string" &&
                Array.isArray(item.efforts) &&
                item.efforts.length &&
                item.efforts.every((level) => typeof level === "string"),
            )
          : [];
        if (catalog.length && !current())
          chosen.model =
            catalog.find((item) => item.model === data.default_model)?.model ?? catalog[0].model;
        const item = current();
        if (item && !item.efforts.includes(chosen.effort))
          chosen.effort = item.efforts.includes(data.default_effort)
            ? data.default_effort
            : item.efforts[0];
        paint();
      },
      selection() {
        return catalog.length ? { ...chosen } : {};
      },
      available() {
        return !catalog.length || Boolean(current()?.available);
      },
      setBusy(value) {
        if (busy === value) return;
        busy = value;
        paint();
      },
    };
  };
})();
