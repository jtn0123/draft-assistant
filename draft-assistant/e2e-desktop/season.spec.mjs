/**
 * The desktop app itself: launched from its own binary, talking to Sleeper
 * through the real Rust backend, driven by WebdriverIO.
 *
 * Everything in `e2e/` replays a recorded dump in a browser. This is the
 * only test that proves the two halves are wired to each other.
 */
describe("the desktop app", () => {
  it("opens on the season screen with this week's matchup", async () => {
    const banner = await browser.$('[aria-label*="summary"]');
    await banner.waitForDisplayed({ timeout: 90_000 });
    const text = await banner.getText();
    // Week, opponent, both projections, and the odds — the four things the
    // banner exists to say.
    expect(text).toMatch(/vs \S+/);
    expect(text).toMatch(/\d+\.\d\s*–\s*\d+\.\d/);
    expect(text).toMatch(/\d+% to win/);
    expect(text).toMatch(/calibrated on last season/);
  });

  it("switches to the draft board and back", async () => {
    const draft = await browser.$('button[aria-label="Draft screen"]');
    await draft.click();
    const board = await browser.$('table');
    await board.waitForDisplayed({ timeout: 20_000 });
    const season = await browser.$('button[aria-label="Season screen"]');
    await season.click();
    await (await browser.$('[aria-label*="summary"]')).waitForDisplayed({ timeout: 20_000 });
  });

  it("prices a trade with a draft pick through the real backend", async () => {
    // The one path the browser suite cannot reach: the preview refuses to
    // price, so only here does a verdict come back from Rust.
    const form = await browser.$(".trade-offer");
    await form.waitForExist({ timeout: 20_000 });
    await (await form.$("summary")).click();
    // One chip row per side; [0] is what I give, [1] what I get.
    const chips = await form.$$('button[aria-label^="Round 1 pick"]');
    await chips[1].waitForDisplayed({ timeout: 10_000 });
    await chips[1].click();
    await (await browser.$("button=Price it")).click();
    const verdict = await browser.$(".verdict");
    await verdict.waitForDisplayed({ timeout: 30_000 });
    const text = await verdict.getText();
    // A first-round pick for nothing: the backend prices the round and the
    // verdict counts it. Both halves of the app, in one assertion.
    expect(text).toMatch(/R1 in \(\+\d+\)/);
    expect(text).toMatch(/they pay next season/);
    expect(text).toMatch(/Both sides gain|You lose on it|They lose on it/);
  });
});
