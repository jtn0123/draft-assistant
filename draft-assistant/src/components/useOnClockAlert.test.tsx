import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useOnClockAlert } from "./useOnClockAlert";

function Harness(props: { onClock: boolean; currentPick?: number; enabled: boolean }) {
  useOnClockAlert(props);
  return null;
}

let started: number;

beforeEach(() => {
  started = 0;
  // A stand-in for the audio device: count how many tones get scheduled.
  const osc = () => ({
    frequency: { value: 0 },
    type: "",
    connect: () => ({ connect: () => undefined }),
    start: () => (started += 1),
    stop: () => undefined,
  });
  const gain = () => ({
    gain: {
      setValueAtTime: () => undefined,
      exponentialRampToValueAtTime: () => undefined,
    },
    connect: () => ({ connect: () => undefined }),
  });
  vi.stubGlobal(
    "AudioContext",
    class {
      currentTime = 0;
      destination = {};
      createOscillator = osc;
      createGain = gain;
      close = () => Promise.resolve();
    },
  );
  document.title = "Draft Assistant";
});

afterEach(() => vi.unstubAllGlobals());

describe("useOnClockAlert", () => {
  it("chimes once when your pick comes up and puts it in the window title", () => {
    const { rerender } = render(<Harness onClock={false} currentPick={26} enabled />);
    expect(started).toBe(0);
    expect(document.title).toBe("Draft Assistant");

    rerender(<Harness onClock currentPick={27} enabled />);
    expect(started, "a two-tone chime").toBe(2);
    expect(document.title).toContain("YOUR PICK 27");

    // Polling re-renders every three seconds; it must not chime again.
    rerender(<Harness onClock currentPick={27} enabled />);
    expect(started).toBe(2);

    // Off the clock, the title goes back to normal.
    rerender(<Harness onClock={false} currentPick={28} enabled />);
    expect(document.title).toBe("Draft Assistant");
  });

  it("chimes again at your next pick, but never when muted", () => {
    const { rerender } = render(<Harness onClock currentPick={27} enabled />);
    expect(started).toBe(2);
    rerender(<Harness onClock={false} currentPick={28} enabled />);
    rerender(<Harness onClock currentPick={30} enabled />);
    expect(started, "a fresh pick chimes").toBe(4);

    rerender(<Harness onClock={false} currentPick={31} enabled={false} />);
    rerender(<Harness onClock currentPick={55} enabled={false} />);
    expect(started, "muted").toBe(4);
    expect(document.title).toContain("YOUR PICK 55");
  });

  it("survives a webview with no audio at all", () => {
    vi.stubGlobal("AudioContext", undefined);
    expect(() => render(<Harness onClock currentPick={2} enabled />)).not.toThrow();
    expect(document.title).toContain("YOUR PICK 2");
  });
});
