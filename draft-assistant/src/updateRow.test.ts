import { describe, expect, it } from "vitest";
import {
  failed,
  firstLine,
  IDLE,
  selection,
  settled,
  started,
  updateRow,
  type UpdateState,
} from "./updateRow";

const noop = () => {};

// The plugin was registered and granted for a whole release cycle and nothing
// in the UI called it: a signed release could reach nobody. This is the row
// that calls it, so every state a user can see it in is pinned here.
describe("the update row's states", () => {
  it("starts as an offer to check, naming the version it would move from", () => {
    const row = updateRow("0.2.0", IDLE, noop);
    expect(row.label).toBe("Check for updates");
    expect(row.note).toContain("v0.2.0");
    expect(row.value).toBe("Check");
    expect(row.on).toBe(false);
  });

  it("checks when chosen from idle, and shows that it is checking", () => {
    expect(selection(IDLE)).toBe("check");
    const row = updateRow("0.2.0", started(IDLE), noop);
    expect(row.label).toBe("Check for updates");
    expect(row.note).toMatch(/Asking/);
    expect(row.value).toBe("…");
  });

  it("does nothing while a check is already running", () => {
    const checking = started(IDLE);
    expect(selection(checking)).toBe("none");
    expect(started(checking)).toBe(checking);
  });

  it("says up to date, with the version, when the feed has nothing newer", () => {
    const state = settled({ current: "0.2.0", available: null, notes: null });
    const row = updateRow("0.2.0", state, noop);
    expect(row.label).toBe("Up to date");
    expect(row.value).toBe("v0.2.0");
    expect(row.on).toBe(true);
    // Up to date is not the end of the road: the next release may land
    // while the menu is open.
    expect(selection(state)).toBe("check");
  });

  it("offers the newer version with the first line of its notes", () => {
    const state = settled({
      current: "0.2.0",
      available: "0.3.1",
      notes: "## 0.3.1\n\n- Keeper floor guard in pre_draft\n- Season loop hardening",
    });
    const row = updateRow("0.2.0", state, noop);
    expect(row.label).toBe("Update to 0.3.1");
    expect(row.note).toBe("Keeper floor guard in pre_draft");
    expect(row.value).toBe("Install");
    expect(row.on).toBe(true);
  });

  it("installs when chosen with an update available, and says the app will restart", () => {
    const available = settled({ current: "0.2.0", available: "0.3.1", notes: null });
    expect(selection(available)).toBe("install");
    const installing = started(available);
    expect(installing).toEqual({ kind: "installing", version: "0.3.1" });
    const row = updateRow("0.2.0", installing, noop);
    expect(row.label).toBe("Update to 0.3.1");
    expect(row.note).toMatch(/restarts on its own/);
    expect(selection(installing)).toBe("none");
  });

  it("falls back to a plain note when a release carries no notes", () => {
    const state = settled({ current: "0.2.0", available: "0.3.1", notes: "  \n\n" });
    expect(updateRow("0.2.0", state, noop).note).toMatch(/Select to download it/);
  });

  it("shows the backend's sentence on failure and offers a retry that checks again", () => {
    const state = failed(new Error("No release feed yet. Nothing has been published to update to"));
    const row = updateRow("0.2.0", state, noop);
    expect(row.label).toBe("Check for updates");
    expect(row.note).toBe("No release feed yet. Nothing has been published to update to");
    expect(row.value).toBe("Retry");
    expect(selection(state)).toBe("check");
    expect(started(state)).toEqual({ kind: "checking" });
  });

  it("never shows [object Object] or a blank note for an unshaped rejection", () => {
    expect(failed({ code: 7 })).toEqual({
      kind: "failed",
      message: "The update check failed. Try again",
    });
    expect(failed("")).toEqual({ kind: "failed", message: "The update check failed. Try again" });
    // Tauri rejects with the command's `Err(String)` as a bare string.
    expect(failed("Could not reach the update server")).toEqual({
      kind: "failed",
      message: "Could not reach the update server",
    });
  });

  it("hands the shell's action to every state, so the menu never needs to know which", () => {
    const states: UpdateState[] = [
      IDLE,
      { kind: "checking" },
      { kind: "current" },
      { kind: "available", version: "0.3.1", notes: null },
      { kind: "installing", version: "0.3.1" },
      { kind: "failed", message: "x" },
    ];
    for (const state of states) {
      expect(updateRow("0.2.0", state, noop).onSelect).toBe(noop);
    }
  });

  it("keeps em-dashes out of every line of copy", () => {
    const states: UpdateState[] = [
      IDLE,
      { kind: "checking" },
      { kind: "current" },
      { kind: "available", version: "0.3.1", notes: null },
      { kind: "installing", version: "0.3.1" },
    ];
    for (const state of states) {
      const row = updateRow("0.2.0", state, noop);
      expect(`${row.label}${row.note}${row.value}`).not.toContain("—");
    }
  });
});

describe("the first line of the release notes", () => {
  it("skips headings, bullets and blank lines to the first words", () => {
    expect(firstLine("# v0.3.1\n\n* First fix\n* Second")).toBe("First fix");
    expect(firstLine("Plain sentence")).toBe("Plain sentence");
  });

  it("is null for no notes or blank notes", () => {
    expect(firstLine(null)).toBeNull();
    expect(firstLine("\n  \n")).toBeNull();
  });
});
