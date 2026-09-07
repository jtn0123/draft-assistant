// The companion phone page as an installed app, and the ways a question can
// fail to send: what `pwa.js` adds, booted for real over `index.html`.

import { afterEach, describe, expect, it } from "vitest";
import {
  boot,
  DEVICE_ID_KEY,
  DEVICE_KEY,
  FakeSocket,
  fakeWakeLock,
  flush,
  okJson,
  setVisibility,
  TOKEN_KEY,
  type Booted,
} from "./test/companionPageHarness";

afterEach(() => {
  document.body.innerHTML = "";
  setVisibility("visible");
});

const drafting = {
  draft: { status: "drafting", current_pick: 12, current_round: 2, on_clock_slot: 3 },
  recommendations: [],
  recent_picks: [],
  my_roster: { players: [], open_starters: [] },
  data_health: { board_size: 300 },
};

/** The page on the Chat tab, connected, with the input holding a question. */
async function asking(
  fetch: Parameters<typeof boot>[0],
  options: Parameters<typeof boot>[1] = {},
): Promise<Booted & { input: HTMLInputElement; submit: () => void }> {
  const booted = boot(fetch, options);
  await flush();
  FakeSocket.instances[0]?.open();
  (document.querySelector('[data-tab="chat"]') as HTMLButtonElement).click();
  const input = booted.byId("chat-input") as HTMLInputElement;
  input.value = "is Rob's trade fair?";
  const submit = () =>
    void booted.byId("chat-form").dispatchEvent(new Event("submit", { cancelable: true }));
  return { ...booted, input, submit };
}

describe("a question the host did not take", () => {
  it("stays in the box when the host answers with an error, with the note beside it", async () => {
    // The failure this prevents: the box was emptied before the request and
    // refilled only when the network threw, so a 409 or a 500 left a note
    // beside an empty box and the question gone.
    const { input, byId, submit } = await asking((path) =>
      path === "/api/chat" ? okJson({}, 409) : okJson(null),
    );
    submit();
    await flush();
    expect(input.value).toBe("is Rob's trade fair?");
    expect(byId("chat-note").textContent).toBe("The host is still answering.");
  });

  it("stays in the box when the host never answers, and the note says how long it waited", async () => {
    const { input, byId, submit, fireTimers } = await asking(
      (path, init) =>
        path === "/api/chat"
          ? new Promise((_resolve, reject) => {
              init?.signal?.addEventListener("abort", () => reject(new Error("aborted")));
            })
          : okJson(null),
      { abortable: true },
    );
    submit();
    await flush();
    // Still waiting: nothing has been decided about the question.
    expect(input.value).toBe("is Rob's trade fair?");
    expect(byId("chat-note").hidden).toBe(true);
    // The deadline runs out.
    fireTimers();
    await flush();
    expect(input.value).toBe("is Rob's trade fair?");
    expect(byId("chat-note").textContent).toBe("The host did not answer in time.");
  });

  it("leaves the box only once the host has taken it", async () => {
    const { input, byId, submit } = await asking((path) =>
      path === "/api/chat" ? okJson({ entry_id: "e9" }, 202) : okJson(null),
    );
    submit();
    await flush();
    expect(input.value).toBe("");
    expect(byId("chat-note").hidden).toBe(true);
  });
});

describe("the screen wake lock while the phone is face down", () => {
  it("is not asked for again until the page is looked at", async () => {
    const wakeLock = fakeWakeLock();
    boot(() => okJson(null), { wakeLock });
    await flush();
    const socket = FakeSocket.instances[0];
    if (!socket) throw new Error("no socket");
    socket.open();
    socket.frame("draft-updated", drafting);
    await flush();
    expect(wakeLock.request).toHaveBeenCalledTimes(1);
    // The screen goes dark: the browser lets the lock go and hides the page.
    setVisibility("hidden");
    wakeLock.sentinels[0]?.releasedByBrowser();
    // The failure this prevents: the pick clock repaints every second, each
    // repaint asked for the lock again, and a hidden page is refused every
    // time. Three repaints here stand in for the whole draft.
    for (let tick = 0; tick < 3; tick += 1) {
      socket.frame("poll-health", { consecutive_failures: 0 });
      await flush();
    }
    expect(wakeLock.request).toHaveBeenCalledTimes(1);
    // The phone is picked up: one request, on the visibility change itself.
    setVisibility("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(wakeLock.request).toHaveBeenCalledTimes(2);
  });
});

describe("pairing as the same phone from an installed copy", () => {
  it("takes the identity from the address when storage is empty, and sends it", async () => {
    // iOS gives a home-screen web app a storage of its own: the token, id
    // and name Safari saved never reach it. The address it was installed
    // from carries the id and the name.
    const { byId, fetch, stored } = boot(() => okJson(null), {
      saved: {},
      search: "?device=dev-9&name=Rob%27s+iPhone",
    });
    expect((byId("pair-device") as HTMLInputElement).value).toBe("Rob's iPhone");
    fetch.mockImplementation((path) =>
      path === "/api/pair"
        ? okJson({ token: "tok-2", host_name: "Justin's Mac", device_id: "dev-9" })
        : okJson(null),
    );
    (byId("pair-code") as HTMLInputElement).value = "424242";
    byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await flush();
    const pairing = fetch.mock.calls.find(([path]) => path === "/api/pair");
    if (!pairing) throw new Error("nothing was sent to /api/pair");
    const body = JSON.parse((pairing[1] as { body: string }).body) as Record<string, unknown>;
    // The failure this prevents: with no id to send, the host listed the
    // installed copy as "iPhone 2" beside the Safari pairing.
    expect(body.device_id).toBe("dev-9");
    expect(body.device_name).toBe("Rob's iPhone");
    expect(stored(TOKEN_KEY)).toBe("tok-2");
  });

  it("does not let the address overrule an identity the page already has", () => {
    const { stored } = boot(() => okJson(null), {
      saved: { [TOKEN_KEY]: "tok-1", [DEVICE_ID_KEY]: "dev-1", [DEVICE_KEY]: "Rob's iPhone" },
      search: "?device=dev-9&name=Somebody+else",
    });
    expect(stored(DEVICE_ID_KEY)).toBe("dev-1");
    expect(stored(DEVICE_KEY)).toBe("Rob's iPhone");
  });

  it("writes the identity into the address after pairing, and never the token", async () => {
    const { byId, address } = boot(
      (path) =>
        path === "/api/pair"
          ? okJson({ token: "tok-2", host_name: "Justin's Mac", device_id: "dev-9" })
          : okJson(null),
      { saved: {} },
    );
    (byId("pair-device") as HTMLInputElement).value = "Rob's iPhone";
    (byId("pair-code") as HTMLInputElement).value = "424242";
    byId("pair-form").dispatchEvent(new Event("submit", { cancelable: true }));
    await flush();
    expect(address()).toBe("/?device=dev-9&name=Rob%27s+iPhone");
    expect(address()).not.toContain("tok-2");
  });

  it("gives a phone paired before the address carried anything its identity too", async () => {
    const { address } = boot(() => okJson(null), {
      saved: { [TOKEN_KEY]: "tok-1", [DEVICE_ID_KEY]: "dev-1", [DEVICE_KEY]: "Rob's iPhone" },
    });
    await flush();
    expect(address()).toBe("/?device=dev-1&name=Rob%27s+iPhone");
  });
});

describe("the manifest link", () => {
  it("is added where the installed app shares the browser's storage", () => {
    boot(() => okJson(null));
    const link = document.head.querySelector('link[rel="manifest"]');
    expect(link?.getAttribute("href")).toBe("/static/manifest.webmanifest");
  });

  it("is left out on iOS, whose installed app would open at the manifest's start_url instead", () => {
    // `navigator.standalone` exists only on iOS Safari; its value says
    // whether the page is already installed, its presence which browser.
    boot(() => okJson(null), { standalone: false });
    expect(document.head.querySelector('link[rel="manifest"]')).toBeNull();
  });
});
