import { config as smoke } from "./wdio.conf";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";

if (!process.env.REHEARSAL_URL || !process.env.REHEARSAL_ARTIFACTS) {
  throw new Error("Use npm run test:e2e:rehearsal to provide isolated data and local fixtures");
}
export const config: WebdriverIO.Config = {
  ...smoke,
  specs: [fileURLToPath(new URL("specs/rehearsal.spec.ts", import.meta.url))],
  mochaOpts: { ui: "bdd", timeout: 180_000 },
  afterTest: async (_test, _context, { passed }) => {
    if (!passed)
      await browser.saveScreenshot(resolve(process.env.REHEARSAL_ARTIFACTS!, "failure.png"));
  },
};
