// At the app's 1000px minimum width the chat column moves below the board, a
// full screen further down than the button that opens it. Nothing on screen
// changed when the panel appeared, so the button read as broken and people
// pressed it again — closing the panel they had just been given.

import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { STACKED_QUERY, useRevealOnMount } from "./useRevealOnMount";

const scrollIntoView = vi.fn();

/** Answer `matches` for the stacked breakpoint and nothing else, the way a
 *  narrow window does. */
function widthIs(stacked: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({ matches: stacked && query === STACKED_QUERY })),
  );
}

function Panel() {
  const ref = useRevealOnMount<HTMLDivElement>();
  return <div ref={ref}>Ask Claude</div>;
}

afterEach(() => {
  vi.unstubAllGlobals();
  scrollIntoView.mockClear();
});

describe("revealing a panel that opened off screen", () => {
  it("scrolls the panel into view in the stacked layout", () => {
    widthIs(true);
    Element.prototype.scrollIntoView = scrollIntoView;
    render(<Panel />);
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
  });

  it("leaves the page alone when the panel is already beside the board", () => {
    widthIs(false);
    Element.prototype.scrollIntoView = scrollIntoView;
    render(<Panel />);
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("renders without matchMedia, which older webviews and jsdom may not have", () => {
    vi.stubGlobal("matchMedia", undefined);
    Element.prototype.scrollIntoView = scrollIntoView;
    const { getByText } = render(<Panel />);
    expect(getByText("Ask Claude")).toBeInTheDocument();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("renders where scrollIntoView is not implemented", () => {
    widthIs(true);
    // jsdom leaves this undefined; the hook must not be the thing that throws.
    Reflect.deleteProperty(Element.prototype, "scrollIntoView");
    const { getByText } = render(<Panel />);
    expect(getByText("Ask Claude")).toBeInTheDocument();
  });
});
