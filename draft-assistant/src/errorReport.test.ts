// The failures this guards are the two that used to leave no trace at all — a
// script error and a rejected promise — and the two ways a reporter makes
// things worse: reporting the same thing for ever, and looping on its own
// failure.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { installErrorReporting, reportError, resetErrorReporting } from "./errorReport";
import { harness } from "./test/appHarness";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

const { api, reset } = harness();

beforeEach(() => {
  reset();
  resetErrorReporting();
});

describe("the frontend error reporter", () => {
  it("sends a rejected promise nobody caught", () => {
    const stop = installErrorReporting();
    window.dispatchEvent(
      Object.assign(new Event("unhandledrejection"), {
        reason: new TypeError("Cannot read properties of undefined"),
      }),
    );
    stop();

    expect(api.logFrontendError).toHaveBeenCalledWith(
      "TypeError: Cannot read properties of undefined",
      "unhandledrejection",
    );
  });

  it("sends a script error with the file and line it came from", () => {
    const stop = installErrorReporting();
    window.dispatchEvent(
      Object.assign(new Event("error"), {
        message: "boom",
        filename: "/assets/season-abc.js",
        lineno: 42,
        error: new Error("boom"),
      }),
    );
    stop();

    expect(api.logFrontendError).toHaveBeenCalledWith("Error: boom", "/assets/season-abc.js:42");
  });

  it("sends the same failure once, however often it happens", () => {
    for (let i = 0; i < 5; i += 1) reportError("Error: the same thing", "render");
    expect(api.logFrontendError).toHaveBeenCalledTimes(1);
  });

  it("stops after a cap so a failing render cannot fill the log", () => {
    for (let i = 0; i < 50; i += 1) reportError(`Error: number ${i}`, "render");
    expect(api.logFrontendError).toHaveBeenCalledTimes(20);
  });

  it("does not throw when the backend refuses the report", () => {
    api.logFrontendError.mockRejectedValue(new Error("no backend"));
    // A reporter that raises here turns one page error into an unhandled
    // rejection, which is the very thing being reported. That is the loop.
    expect(() => reportError("Error: something", "render")).not.toThrow();
  });

  it("stops listening once the app says so", () => {
    const stop = installErrorReporting();
    stop();
    window.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "late" }));
    expect(api.logFrontendError).not.toHaveBeenCalled();
  });

  it("trims a runaway message rather than writing a novel to the log", () => {
    reportError("x".repeat(5000), "render");
    const sent = String(api.logFrontendError.mock.calls[0]?.[0]);
    expect(sent.length).toBe(800);
  });
});
