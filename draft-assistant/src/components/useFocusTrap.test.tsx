import { useRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useFocusTrap } from "./useFocusTrap";

function Dialog({ empty = false }: { empty?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, vi.fn());
  return (
    <div ref={ref} role="dialog" aria-label="Test" tabIndex={-1}>
      <button disabled>Disabled first</button>
      <div hidden>
        <button>Hidden</button>
      </div>
      <div style={{ display: "none" }}>
        <button>CSS hidden</button>
      </div>
      {!empty && <button>Cancel</button>}
      <button disabled>Disabled last</button>
    </div>
  );
}
it("wraps around enabled visible controls when both ends are disabled", () => {
  render(<Dialog />);
  const cancel = screen.getByRole("button", { name: "Cancel" });
  cancel.focus();
  expect(fireEvent.keyDown(cancel, { key: "Tab" })).toBe(false);
  expect(cancel).toHaveFocus();
  expect(fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true })).toBe(false);
  expect(cancel).toHaveFocus();
});
it("keeps focus on the dialog when no eligible controls remain", () => {
  render(<Dialog empty />);
  const dialog = screen.getByRole("dialog");
  dialog.focus();
  expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
  expect(dialog).toHaveFocus();
});
