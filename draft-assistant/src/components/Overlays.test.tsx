import { useRef, useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog, Toast } from "./Overlays";
import { useFocusTrap } from "./useFocusTrap";
import type { Platform } from "../types";
import { settle } from "../test/settle";

/** The dialog as the app puts it on screen: a shell, and an opener inside it. */
function Harness({
  onConfirm = () => {},
  platform = "sleeper",
}: {
  onConfirm?: () => void;
  platform?: Platform;
}) {
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
          platform={platform}
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

describe("naming the service", () => {
  it("says Yahoo on a Yahoo draft rather than Sleeper for everything", async () => {
    render(<Harness platform="yahoo" />);
    await settle(() => screen.getByRole("button", { name: "Draft" }).click());

    const note = screen.getByText(/does not draft them in/);
    expect(note).toHaveTextContent("does not draft them in Yahoo");
    expect(note).toHaveTextContent("live sync from Yahoo overrides it");
    expect(note).not.toHaveTextContent("Sleeper");
  });
});

/** The dialog as the first-launch screen puts it on screen: no `.shell`
 *  anywhere, because the app has no league yet and never renders one. */
function SetupHarness() {
  const dialog = useRef<HTMLDivElement>(null);
  useFocusTrap(dialog, () => undefined);
  return (
    <div className="app">
      <div className="setup">
        <button type="button">Load league</button>
      </div>
      <div className="scrim" role="presentation">
        <div className="dialog" ref={dialog} role="dialog" aria-modal="true">
          <button type="button">Join</button>
        </div>
      </div>
    </div>
  );
}

describe("what the trap holds inert", () => {
  it("covers a screen that has no shell to look for", () => {
    // The trap used to look for `.shell`, which only the running app renders.
    // On the very first launch — the one screen where the dialogs are the only
    // thing to interact with — it found nothing and quietly did nothing.
    render(<SetupHarness />);
    expect(document.querySelector(".setup")).toHaveAttribute("inert");
    expect(document.querySelector(".dialog")).not.toHaveAttribute("inert");
  });

  it("gives the page back when the dialog closes", async () => {
    await openDialog();
    expect(document.querySelector(".shell")).toHaveAttribute("inert");

    await settle(() => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    expect(document.querySelector(".shell")).not.toHaveAttribute("inert");
  });
});

describe("marking a player drafted twice", () => {
  it("stops answering while the pick it already sent is out", () => {
    // The dialog stayed live through the call, so a double tap sent two
    // identical picks and the shell showed the refusal of the second as a
    // failure of a pick that had in fact gone through.
    const onConfirm = vi.fn();
    render(
      <div className="app">
        <div className="shell" />
        <ConfirmDialog
          pickLabel="Pick 3.07 · slot 7"
          playerName="Josh Downs"
          platform="sleeper"
          busy
          onConfirm={onConfirm}
          onCancel={() => undefined}
        />
      </div>,
    );

    const button = screen.getByRole("button", { name: "Marking…" });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
