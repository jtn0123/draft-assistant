import { browser, $, expect } from "@wdio/globals";

/**
 * The one thing no other test in this repo can see: the app actually starting.
 *
 * `npm run verify` tests the React bundle in jsdom and the Rust by calling
 * functions directly. Neither ever loads the built bundle into a WKWebView
 * under the production CSP, and neither sends an `invoke()` over the real IPC
 * bridge. That seam is what this covers, and it is the seam that produces the
 * one failure a user actually sees: a window that opens blank.
 *
 * So the assertion is deliberately about *reaching a resolved screen*, not
 * about any particular league's numbers. This machine has a league saved and
 * a fresh checkout or a CI runner does not; both are legitimate, and a test
 * that only passed on one of them would be a test of the tester's laptop.
 * What is not legitimate -- and what fails here -- is the app never getting
 * past its launch state:
 *
 *   - a blank window (CSP rejected a chunk, so React never mounted)
 *   - the launch screen hanging forever (`get_config` never answered, i.e.
 *     a command is missing from `generate_handler!` or the capability set
 *     does not permit the call)
 *   - a hard crash on boot
 */
describe("Draft Assistant", () => {
  it("boots into a resolved screen with real content on it", async () => {
    // React mounted at all. Before this, the window is genuinely empty.
    const root = await $(".app");
    await root.waitForExist();

    // The app starts on `LaunchScreen` and leaves it in exactly one of two
    // directions, depending on whether a league is configured. Waiting for
    // "either" is what makes this honest on both a configured Mac and a
    // clean CI runner -- and it still fails if it leaves in neither.
    //
    //   .app-header          -> a league restored; the full shell is up
    //   .card-screen-submit  -> no league; setup is offering to add one
    const resolved = await browser.waitUntil(
      async () => {
        for (const selector of [".app-header", ".card-screen-submit"]) {
          if (await $(selector).isDisplayed()) return selector;
        }
        return false;
      },
      {
        timeout: 90_000,
        interval: 500,
        timeoutMsg:
          "the app never got past its launch screen: React mounted but " +
          "neither the header nor the setup form appeared",
      },
    );

    // Something is painted, not just present. A DOM full of nodes that
    // render to nothing is the signature of a CSS chunk the CSP blocked.
    const text = (await root.getText()).trim();
    // The point of a smoke test is to show what it saw; a green tick with
    // no evidence is worth less.
    console.log(`[e2e] resolved on ${String(resolved)}; screen reads:\n${text.slice(0, 400)}`);
    expect(text.length).toBeGreaterThan(20);

    if (resolved === ".app-header") {
      // The shell only renders once a `DraftView` came back over IPC, so
      // reaching here already proves the round trip. Assert the two things
      // that view puts on screen: the league's name in the <h1>, and the
      // screen toggle the header is built around.
      await expect($(".app-header h1")).toHaveText(expect.stringMatching(/\S/));
      await expect($(".mode-toggle")).toBeDisplayed();
    } else {
      // No league configured. `get_config` still had to answer for the app
      // to get here, and the setup form is the real screen it shows.
      await expect($(".card-screen h1")).toHaveText("Draft Assistant");
    }
  });
});
