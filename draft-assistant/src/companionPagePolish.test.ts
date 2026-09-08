import { afterEach, expect, it } from "vitest";
import { boot, FakeSocket, flush, okJson } from "./test/companionPageHarness";

afterEach(() => {
  document.body.innerHTML = "";
});

it("suggestions fill and focus the composer without sending an AI request", async () => {
  const page = boot(() => okJson(null));
  await flush();
  document.querySelector<HTMLButtonElement>('[data-tab="chat"]')?.click();
  const suggestion = document.querySelector<HTMLButtonElement>(
    '#chat-block button[data-question="Who should I draft next?"]',
  );
  if (!suggestion) throw new Error("The shipped Chat page has no next-pick suggestion");
  page.fetch.mockClear();
  suggestion.click();
  expect((page.byId("chat-input") as HTMLInputElement).value).toBe("Who should I draft next?");
  expect(document.activeElement).toBe(page.byId("chat-input"));
  expect(page.fetch).not.toHaveBeenCalled();
});

it("shows Connecting while pairing, then restores retry controls and explains a wrong code", async () => {
  let answer!: (response: Response) => void;
  const page = boot(
    () =>
      new Promise<Response>((resolve) => {
        answer = resolve;
      }),
    { saved: {} },
  );
  const button = page.byId("pair-submit") as HTMLButtonElement;
  page.byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
  expect(button.textContent).toBe("Connecting…");
  expect(button.disabled).toBe(true);
  answer(new Response(JSON.stringify({ error: "wrong code" }), { status: 403 }));
  await flush();
  expect(button.textContent).toBe("Connect");
  expect(button.disabled).toBe(false);
  expect(page.byId("pair-error").textContent).toContain("current six-digit code");
  page.byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
  expect(page.byId("pair-error").hidden).toBe(true);
  answer(new Response("{}", { status: 429 }));
  await flush();
  expect(page.byId("pair-error").textContent).toContain("Wait a minute");
});

it("shows a retryable connection error when the pairing request fails", async () => {
  const page = boot(() => Promise.reject(new Error("offline")), { saved: {} });
  page.byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  expect(page.byId("pair-error").textContent).toContain("Check your connection and try again");
  expect((page.byId("pair-submit") as HTMLButtonElement).disabled).toBe(false);
});

it.each([
  ["pre_draft", 2, [1, 2], false, "Draft has not started"],
  ["pre_draft", 3, [1, 2], false, "Pick 4"],
  ["paused", 3, [], true, "Draft paused"],
  ["complete", 8, [], false, "Draft complete"],
])(
  "announces %s without inventing an active turn",
  async (status, picks, keepers, paused, label) => {
    const page = boot(() => okJson(null));
    await flush();
    FakeSocket.instances[0]?.open();
    FakeSocket.instances[0]?.frame("draft-updated", {
      draft: {
        status,
        total_picks_made: picks,
        keeper_picks: keepers,
        paused,
        current_pick: 4,
        current_round: 2,
        is_my_pick: true,
        on_clock_slot: 1,
      },
      recommendations: [],
      my_roster: { players: [], open_starters: [] },
      data_health: { board_size: 300 },
    });
    expect(page.byId("clock-strip").textContent).toContain(label);
    if (label !== "Pick 4") expect(page.byId("clock-strip").textContent).not.toContain("Your pick");
  },
);
