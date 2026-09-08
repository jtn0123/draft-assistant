import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { afterEach, expect, it, vi } from "vitest";
import { boot, okJson } from "./test/companionPageHarness";

const html = readFileSync("src-tauri/companion-static/index.html", "utf8");
const byId = (id: string) => document.getElementById(id) as HTMLElement;

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

it("keeps Connect disabled when JavaScript has not booted", () => {
  document.body.innerHTML = html;
  expect((byId("pair-submit") as HTMLButtonElement).disabled).toBe(true);
  expect(byId("boot-status").textContent).toContain("Loading");
});

it("shows recovery instructions when a dependent script never arrives", () => {
  vi.useFakeTimers();
  document.body.innerHTML = html;
  const events = new EventTarget();
  const sandbox = {
    document,
    window: { setTimeout, addEventListener: events.addEventListener.bind(events) },
  };
  runInContext(readFileSync("src-tauri/companion-static/boot.js", "utf8"), createContext(sandbox));
  // No helpers/clock/pwa/app script executes: the shell still must explain itself.
  vi.advanceTimersByTime(10000);
  expect(byId("boot-status").hidden).toBe(false);
  expect(byId("boot-status").textContent).toContain("Reload");
  expect(byId("boot-status").textContent).toContain("Safari or Chrome");
  expect((byId("pair-submit") as HTMLButtonElement).disabled).toBe(true);
});

it("enables Connect only after the real page installs its working submit handler", async () => {
  const page = boot(() => okJson({}), { saved: {} });
  expect((page.byId("pair-submit") as HTMLButtonElement).disabled).toBe(false);
  expect(page.byId("boot-status").hidden).toBe(true);
  page.byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(page.fetch).toHaveBeenCalledWith("/api/pair", expect.objectContaining({ method: "POST" }));
});
