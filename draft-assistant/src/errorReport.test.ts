// The failures this guards are the two that used to leave no trace at all — a
// script error and a rejected promise — and the two ways a reporter makes
// things worse: reporting the same thing for ever, and looping on its own
// failure.

import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  firstFrames,
  installErrorReporting,
  reportError,
  resetErrorReporting,
} from "./errorReport";
import { harness } from "./test/appHarness";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

const { api, reset } = harness();

beforeEach(() => {
  reset();
  resetErrorReporting();
});

/** An error with a stack of a known shape, whatever engine runs the tests. */
function errorWithStack(message: string, frames: string[]): Error {
  const error = new TypeError(message);
  error.stack = [`TypeError: ${message}`, ...frames.map((frame) => `    ${frame}`)].join("\n");
  return error;
}

describe("the frontend error reporter", () => {
  it("sends a rejected promise nobody caught, with the first frames of its stack", () => {
    const stop = installErrorReporting();
    window.dispatchEvent(
      Object.assign(new Event("unhandledrejection"), {
        reason: errorWithStack("Cannot read properties of undefined", [
          "at loadSeason (http://localhost/assets/season.js:10:5)",
          "at tick (http://localhost/assets/season.js:40:3)",
          "at run (http://localhost/assets/index.js:1:1)",
        ]),
      }),
    );
    stop();

    expect(api.logFrontendError).toHaveBeenCalledWith(
      "TypeError: Cannot read properties of undefined",
      "unhandledrejection",
      "at loadSeason (http://localhost/assets/season.js:10:5)\nat tick (http://localhost/assets/season.js:40:3)",
    );
  });

  it("sends a script error with the file and line it came from", () => {
    const stop = installErrorReporting();
    window.dispatchEvent(
      Object.assign(new Event("error"), {
        message: "boom",
        filename: "/assets/season-abc.js",
        lineno: 42,
        error: errorWithStack("boom", ["at render (/assets/season-abc.js:42:7)"]),
      }),
    );
    stop();

    expect(api.logFrontendError).toHaveBeenCalledWith(
      "TypeError: boom",
      "/assets/season-abc.js:42",
      "at render (/assets/season-abc.js:42:7)",
    );
  });

  // The failure this prevents: a render error reached the log as
  // "frontend: TypeError: x is undefined where=render", which names no screen
  // and no component. The ErrorBoundary hands React's component stack here.
  it("forwards a component stack's first two frames and no more", () => {
    reportError(
      "Error: chunk missing",
      "render",
      "\n    at Board (http://localhost/assets/index.js:10:5)\n    at div\n    at Panel\n    at App",
    );
    expect(api.logFrontendError).toHaveBeenCalledWith(
      "Error: chunk missing",
      "render",
      "at Board (http://localhost/assets/index.js:10:5)\nat div",
    );
  });

  it("sends no stack argument at all when the caller has none", () => {
    reportError("Error: no stack", "render");
    expect(api.logFrontendError).toHaveBeenCalledWith("Error: no stack", "render");
    expect(api.logFrontendError.mock.calls[0]).toHaveLength(2);
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

  it("trims a runaway stack frame the same way", () => {
    reportError(
      "Error: long frame",
      "render",
      `at f (data:text/javascript;base64,${"A".repeat(5000)})`,
    );
    const sent = String(api.logFrontendError.mock.calls[0]?.[2]);
    expect(sent.length).toBe(400);
  });
});

describe("the first frames of a stack", () => {
  it("drops the message line a V8 stack opens with, so it is not stored twice", () => {
    expect(
      firstFrames("Error: boom\n    at a (http://x/a.js:1:1)\n    at b (http://x/b.js:2:2)"),
    ).toBe("at a (http://x/a.js:1:1)\nat b (http://x/b.js:2:2)");
  });

  it("reads a WebKit stack, whose frames have no 'at'", () => {
    expect(firstFrames("a@http://x/a.js:1:1\nb@http://x/b.js:2:2\nc@http://x/c.js:3:3")).toBe(
      "a@http://x/a.js:1:1\nb@http://x/b.js:2:2",
    );
  });

  it("is nothing for a stack with no frames in it", () => {
    expect(firstFrames(undefined)).toBeUndefined();
    expect(firstFrames("")).toBeUndefined();
    expect(firstFrames("Error: only a message")).toBeUndefined();
  });
});
