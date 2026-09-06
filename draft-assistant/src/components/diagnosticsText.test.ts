// What "Copy diagnostics" puts on the clipboard, and the one line of it that
// is about this machine's clock rather than the app.

import { describe, expect, it } from "vitest";
import { diagnostics } from "../test/appHarness";
import { diagnosticsText, localTimestamp, utcOffset } from "./diagnosticsText";

describe("the local time anchor", () => {
  // The failure this prevents: every log line is UTC, and the paste had no
  // local time anywhere, so "it broke around eight" had to be converted by
  // hand by whoever read it, on whatever day they guessed.
  it("is the second line of the copied text, with the offset spelled out", () => {
    const at = new Date(2026, 8, 5, 20, 1, 2);
    const lines = diagnosticsText(diagnostics(), "0.2.0", at).split("\n");
    expect(lines[1]).toBe(
      `Copied at: ${localTimestamp(at)} (local time, UTC${utcOffset(at)}; log lines are UTC)`,
    );
    expect(lines[1]).toContain("2026-09-05T20:01:02");
  });

  it("writes the wall clock, not UTC, in the shape the log lines use", () => {
    const at = new Date(2026, 0, 9, 7, 3, 4);
    const stamp = localTimestamp(at);
    expect(stamp).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
    expect(stamp.startsWith("2026-01-09T07:03:04")).toBe(true);
    expect(stamp.endsWith(utcOffset(at))).toBe(true);
  });

  it("signs the offset the way a timestamp does, not the way JavaScript does", () => {
    const at = new Date(2026, 6, 1);
    const minutes = -at.getTimezoneOffset();
    const expectedSign = minutes < 0 ? "-" : "+";
    const offset = utcOffset(at);
    expect(offset.startsWith(expectedSign)).toBe(true);
    expect(offset).toMatch(/^[+-]\d{2}:\d{2}$/);
    // Whatever zone the tests run in, the offset reads back to the same
    // number of minutes.
    const [hours, mins] = offset.slice(1).split(":").map(Number);
    expect((hours ?? 0) * 60 + (mins ?? 0)).toBe(Math.abs(minutes));
  });
});

describe("the copied text", () => {
  it("still opens with the version and keeps the log section last", () => {
    const text = diagnosticsText(
      diagnostics({ log_tail: ["2026-09-05T03:01:00Z INFO app started"] }),
      "0.2.0",
      new Date(2026, 8, 5, 20, 1, 2),
    );
    const lines = text.split("\n");
    expect(lines[0]).toMatch(/^Draft Assistant /);
    expect(lines[lines.length - 1]).toBe("2026-09-05T03:01:00Z INFO app started");
    expect(text).toContain("--- log ---");
  });

  it("uses the moment of the copy when no time is given", () => {
    const before = Date.now();
    const line = diagnosticsText(diagnostics(), "0.2.0").split("\n")[1] ?? "";
    const year = new Date(before).getFullYear();
    expect(line).toContain(`Copied at: ${year}-`);
  });
});
