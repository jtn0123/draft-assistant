import { describe, expect, it, vi } from "vitest";
import type { DraftView } from "./types";

// `api.ts` picks its implementation at import time by sniffing the Tauri
// global, so the flag has to exist before the module is evaluated.
const tauri = vi.hoisted(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  return { listeners: new Map<string, (event: { payload: unknown }) => void>() };
});

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.listeners.set(name, handler);
    return () => undefined;
  }),
}));

import { api } from "./api";

describe("Tauri live-update listener", () => {
  it("delivers a valid view and reports an invalid one instead of dropping it", async () => {
    const views: DraftView[] = [];
    const errors: unknown[] = [];
    await api.onDraftUpdated(
      (view) => views.push(view),
      (error) => errors.push(error),
    );
    const emit = tauri.listeners.get("draft-updated");
    expect(emit).toBeDefined();

    emit?.({ payload: { schema_version: "1.3" } });
    emit?.({ payload: { schema_version: "0.9" } });

    expect(views).toHaveLength(1);
    expect(errors).toHaveLength(1);
    expect(String(errors[0])).toContain("expected schema 1.3, received 0.9");
  });
});
