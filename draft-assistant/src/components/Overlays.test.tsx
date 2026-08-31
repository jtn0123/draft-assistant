import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog, Toast } from "./Overlays";
import { settle } from "../test/settle";

/** The dialog as the app puts it on screen: a shell, and an opener inside it. */
function Harness({ onConfirm = () => {} }: { onConfirm?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="app">
      <div className="shell">
        <button type="button" onClick={() => setOpen(true)}>
          Draft
        </button>
        <button type="button">Somewhere else</button>
      </div>
      {open && (
        <ConfirmDialog
          pickLabel="Pick 3.07 · slot 7"
          playerName="Josh Downs"
          onConfirm={() => {
            setOpen(false);
            onConfirm();
          }}
          onCancel={() => setOpen(false)}
        />
      )}
    </div>
  );
}

const openDialog = async () => {
  render(<Harness />);
  const opener = screen.getByRole("button", { name: "Draft" });
  opener.focus();
  await settle(() => {
    opener.click();
  });
  return opener;
};

describe("the confirm dialog and the keyboard", () => {
  it("opens with the confirming button focused", async () => {
    await openDialog();
    expect(screen.getByRole("button", { name: "Mark drafted" })).toHaveFocus();
  });

  it("keeps Tab inside the dialog instead of leaking to the board behind", async () => {
    await openDialog();
    const user = userEvent.setup();
    const confirm = screen.getByRole("button", { name: "Mark drafted" });
    const cancel = screen.getByRole("button", { name: "Cancel" });

    await user.tab();
    expect(cancel).toHaveFocus();
    // Cancel is the last stop in the dialog: the next Tab wraps round rather
    // than landing on a row hidden behind the scrim.
    await user.tab();
    expect(confirm).toHaveFocus();
    await user.tab({ shift: true });
    expect(cancel).toHaveFocus();
  });

  it("returns focus to whatever opened it when it closes", async () => {
    const opener = await openDialog();
    await settle(() => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("closes on Escape and gives focus back", async () => {
    const opener = await openDialog();
    await settle(() => {
      fireEvent.keyDown(document, { key: "Escape" });
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("holds the app behind inert while it is open, and lets go afterwards", async () => {
    const { container } = render(<Harness />);
    const shell = container.querySelector(".shell");
    expect(shell).not.toHaveAttribute("inert");

    await settle(() => {
      screen.getByRole("button", { name: "Draft" }).click();
    });
    expect(shell).toHaveAttribute("inert");

    await settle(() => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    expect(shell).not.toHaveAttribute("inert");
  });

  it("confirms the pick from the button it opened focused on", async () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    await settle(() => {
      screen.getByRole("button", { name: "Draft" }).click();
    });
    await settle(() => {
      screen.getByRole("button", { name: "Mark drafted" }).click();
    });
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

describe("Toast", () => {
  it("announces itself and can be dismissed", async () => {
    const onDismiss = vi.fn();
    render(<Toast message="Could not record that pick" onDismiss={onDismiss} />);
    expect(screen.getByRole("status")).toHaveTextContent("Could not record that pick");
    await settle(() => {
      screen.getByRole("button", { name: "Dismiss" }).click();
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

describe("a toast with something to do about it", () => {
  it("is announced as an alert and hands the retry back", async () => {
    const onDismiss = vi.fn();
    const onClick = vi.fn();
    render(
      <Toast
        message="Could not mark Josh Downs as drafted — Sleeper is not answering"
        action={{ label: "Try again", onClick }}
        onDismiss={onDismiss}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Could not mark Josh Downs as drafted");
    // Plain messages stay polite; only ones with a decision in them interrupt.
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    await settle(() => {
      screen.getByRole("button", { name: "Try again" }).click();
    });
    expect(onClick).toHaveBeenCalledTimes(1);
    // The failed attempt clears itself out of the way of whatever happens next.
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
