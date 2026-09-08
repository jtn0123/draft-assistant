import { afterEach, expect, it } from "vitest";
import { boot, flush, okJson } from "./test/companionPageHarness";

afterEach(() => {
  document.body.innerHTML = "";
});

it("compact is off by default, flips on tap, and is remembered on the device", async () => {
  const page = boot(() => okJson(null), { saved: { "da.companion.token": "tok" } });
  await flush();
  const root = page.byId("companion-root");
  const toggle = page.byId("compact-toggle");
  expect(root.classList.contains("compact")).toBe(false);
  expect(toggle).toHaveTextContent("Compact off");
  toggle.click();
  expect(root.classList.contains("compact")).toBe(true);
  expect(toggle).toHaveAttribute("aria-pressed", "true");
  expect(page.stored("da.companion.compact")).toBe("on");
  toggle.click();
  expect(root.classList.contains("compact")).toBe(false);
  expect(page.stored("da.companion.compact")).toBe("off");
});

it("comes up compact when the device chose that before", async () => {
  const page = boot(() => okJson(null), {
    saved: { "da.companion.token": "tok", "da.companion.compact": "on" },
  });
  await flush();
  expect(page.byId("companion-root").classList.contains("compact")).toBe(true);
  expect(page.byId("compact-toggle")).toHaveTextContent("Compact on");
});

it("shows the host's note under a model's name and names it for a screen reader", async () => {
  const catalog = {
    default_model: "Opus 5",
    default_effort: "High",
    models: [
      { model: "Opus 5", available: true, efforts: ["High"], note: "" },
      { model: "Fable 5.1", available: true, efforts: ["High"], note: "slower, smarter" },
    ],
  };
  boot((path) => okJson(path === "/api/chat/models" ? catalog : null), {
    saved: { "da.companion.token": "tok" },
  });
  await flush();
  const fable = document.querySelector<HTMLButtonElement>('[data-model="Fable 5.1"]');
  expect(fable?.querySelector(".model-hint")).toHaveTextContent("slower, smarter");
  expect(fable).toHaveAttribute("aria-label", "Fable 5.1, slower, smarter");
  expect(document.querySelector('[data-model="Opus 5"] .model-hint')).toBeNull();
});
