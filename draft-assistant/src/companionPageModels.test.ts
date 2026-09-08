import { afterEach, expect, it } from "vitest";
import { boot, flush, okJson } from "./test/companionPageHarness";

const catalog = {
  default_model: "Opus 5",
  default_effort: "High",
  models: [
    { model: "Opus 5", available: true, efforts: ["Off", "High", "Max"] },
    { model: "Fable 5.1", available: false, efforts: ["Low", "High"] },
    { model: "GPT-6 Astra", available: true, efforts: ["Low", "High", "xhigh", "Max"] },
    { model: "GPT-5.6 Sol", available: true, efforts: ["Off", "Low", "High", "xhigh", "Max"] },
  ],
};
afterEach(() => {
  document.body.innerHTML = "";
});

it("chooses and remembers a host model, collapses, then sends its selected effort", async () => {
  const page = boot((path) => okJson(path === "/api/chat/models" ? catalog : null));
  await flush();
  document.querySelector<HTMLButtonElement>('[data-tab="chat"]')?.click();
  const toggle = page.byId("model-toggle");
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  toggle.click();
  expect(page.byId("model-options")).not.toHaveAttribute("hidden");
  expect(document.querySelector('[data-model="Fable 5.1"]')).toBeDisabled();
  document.querySelector<HTMLButtonElement>('[data-model="GPT-6 Astra"]')?.click();
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(toggle).toHaveFocus();
  const effort = page.byId("model-effort") as HTMLSelectElement;
  effort.value = "Max";
  effort.dispatchEvent(new Event("change"));
  expect(JSON.parse(page.stored("da.companion.model-choice") ?? "{}")).toEqual({
    model: "GPT-6 Astra",
    effort: "Max",
  });
  (page.byId("chat-input") as HTMLInputElement).value = "Who next?";
  page.byId("chat-form").dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  const call = page.fetch.mock.calls.find(
    ([path, init]) => path === "/api/chat" && init?.method === "POST",
  );
  const body = call?.[1]?.body;
  if (typeof body !== "string") throw new Error("The chat request body must be JSON text");
  expect(JSON.parse(body)).toEqual({
    screen: "season",
    text: "Who next?",
    model: "GPT-6 Astra",
    effort: "Max",
  });
});

it("restores device choices and repairs an effort the host no longer supports", async () => {
  const page = boot((path) => okJson(path === "/api/chat/models" ? catalog : null), {
    saved: {
      "da.companion.token": "token",
      "da.companion.model-choice": JSON.stringify({ model: "GPT-6 Astra", effort: "Off" }),
    },
  });
  await flush();
  expect(page.byId("model-toggle")).toHaveTextContent("GPT-6 Astra");
  expect(page.byId("model-effort")).toHaveValue("High");
});

it("keeps old hosts usable without a model catalog and sends no invented choices", async () => {
  const page = boot(() => okJson(null));
  await flush();
  expect(page.byId("model-toggle")).toBeDisabled();
  expect(page.byId("model-toggle")).toHaveAccessibleName("Model: Host default");
  expect(page.byId("model-effort")).toBeDisabled();
  expect(page.byId("model-effort")).toHaveValue("");
  expect(page.byId("model-effort")).toHaveTextContent("Unknown");
  expect(page.byId("model-note")).toHaveTextContent("host will use its default");
});
