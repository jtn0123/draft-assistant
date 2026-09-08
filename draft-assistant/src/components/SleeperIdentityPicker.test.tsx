import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import fixture from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
const api = vi.hoisted(() => ({
  listSleeperMembers: vi.fn(),
  setMyUsername: vi.fn(),
  getState: vi.fn(),
}));
vi.mock("../api", () => ({ api }));
import { SleeperIdentityPicker } from "./SleeperIdentityPicker";
const view = fixture as unknown as DraftView;
beforeEach(() => {
  vi.resetAllMocks();
  api.listSleeperMembers.mockResolvedValue([
    { user_id: "1", display_name: "alex", draft_slot: null, is_current: true },
    { user_id: "2", display_name: "zoe", draft_slot: null, is_current: false },
  ]);
  api.setMyUsername.mockResolvedValue("2");
  api.getState.mockResolvedValue(view);
});
it("lists account names before draft order, saves the account ID, and refreshes the seat", async () => {
  const onSaved = vi.fn();
  render(<SleeperIdentityPicker view={view} onSaved={onSaved} />);
  expect(await screen.findByRole("button", { name: "alex (default)" })).toBeDisabled();
  expect(screen.getAllByText(/Draft seat pending/)).toHaveLength(2);
  await userEvent.click(screen.getByRole("button", { name: "zoe" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(view));
  expect(api.setMyUsername).toHaveBeenCalledWith("2");
  expect(screen.getByRole("button", { name: "zoe (default)" })).toBeDisabled();
});
it("keeps the current account when saving fails and shows the error inline", async () => {
  api.setMyUsername.mockRejectedValue(new Error("Sleeper unavailable"));
  const onSaved = vi.fn();
  render(<SleeperIdentityPicker view={view} onSaved={onSaved} />);
  await userEvent.click(await screen.findByRole("button", { name: "zoe" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Sleeper unavailable");
  expect(screen.getByRole("button", { name: "alex (default)" })).toBeDisabled();
  expect(onSaved).not.toHaveBeenCalled();
});
it("allows a typed username when the member list is unavailable", async () => {
  api.listSleeperMembers.mockRejectedValue(new Error("Member lookup unavailable"));
  render(<SleeperIdentityPicker view={view} onSaved={vi.fn()} />);
  await screen.findByRole("alert");
  await userEvent.type(screen.getByLabelText("Sleeper username"), "my_account");
  await userEvent.click(screen.getByRole("button", { name: "Save username" }));
  await waitFor(() => expect(api.setMyUsername).toHaveBeenCalledWith("my_account"));
});
it("ignores a late member list after switching leagues", async () => {
  let finish!: (value: unknown) => void;
  api.listSleeperMembers.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const { rerender } = render(<SleeperIdentityPicker view={view} onSaved={vi.fn()} />);
  await waitFor(() => expect(api.listSleeperMembers).toHaveBeenCalledTimes(1));
  rerender(
    <SleeperIdentityPicker
      view={{ ...view, league: { ...view.league, league_id: "other" } }}
      onSaved={vi.fn()}
    />,
  );
  await screen.findByRole("button", { name: "zoe" });
  finish([{ user_id: "stale", display_name: "Old account", draft_slot: 1, is_current: false }]);
  await waitFor(() => expect(screen.queryByText("Old account")).not.toBeInTheDocument());
});
