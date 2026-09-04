// Joining a host: what the address box accepts, what a wrong code says, and
// what a successful pair leaves behind.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseHostAddress, readFollow } from "../companion";
import { fakeStorage } from "../test/appHarness";
import { JoinHost } from "./JoinHost";

const fetchMock = vi.fn();

beforeEach(() => {
  fakeStorage();
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function json(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as unknown as Response;
}

describe("what an address can look like", () => {
  it("takes host:port, a bare host, and the URL out of the QR", () => {
    expect(parseHostAddress("192.168.1.5:7878")).toBe("http://192.168.1.5:7878");
    expect(parseHostAddress("  192.168.1.5  ")).toBe("http://192.168.1.5:7878");
    expect(parseHostAddress("http://192.168.1.5:7878/")).toBe("http://192.168.1.5:7878");
    expect(parseHostAddress("http://mac.local:7900/?code=1")).toBe("http://mac.local:7900");
  });

  it("has nothing to make of an empty box", () => {
    expect(parseHostAddress("")).toBeNull();
    expect(parseHostAddress("   ")).toBeNull();
  });
});

async function fill(address: string, code: string, name = "Justin's other Mac") {
  await userEvent.type(screen.getByLabelText("Host address"), address);
  await userEvent.type(screen.getByLabelText("Code"), code);
  const nickname = screen.getByLabelText(/This device.s name/);
  await userEvent.clear(nickname);
  await userEvent.type(nickname, name);
  await userEvent.click(screen.getByRole("button", { name: "Join" }));
}

describe("joining", () => {
  it("pairs, remembers the host, and hands the record on", async () => {
    fetchMock.mockResolvedValue(json({ token: "tok-9", host_name: "Justin's Mac" }));
    const joined = vi.fn();
    render(<JoinHost onClose={() => undefined} onJoined={joined} />);

    await fill("http://192.168.1.5:7878/", "418902");

    await waitFor(() => expect(joined).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledWith(
      "http://192.168.1.5:7878/api/pair",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          code: "418902",
          device_name: "Justin's other Mac",
          kind: "desktop",
        }),
      }),
    );
    expect(readFollow()).toEqual({
      url: "http://192.168.1.5:7878",
      token: "tok-9",
      host_name: "Justin's Mac",
    });
  });

  it("says the code is wrong and remembers nothing", async () => {
    fetchMock.mockResolvedValue(json({ error: "wrong code" }, 403));
    render(<JoinHost onClose={() => undefined} onJoined={() => undefined} />);

    await fill("192.168.1.5:7878", "111111");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "That code is not the one on the host's screen.",
    );
    expect(readFollow()).toBeNull();
    // And the button is usable again, rather than stuck on "Joining…".
    expect(screen.getByRole("button", { name: "Join" })).toBeEnabled();
  });

  it("refuses a code that is not six digits before troubling the host", async () => {
    render(<JoinHost onClose={() => undefined} onJoined={() => undefined} />);
    await fill("192.168.1.5:7878", "4189");
    expect(await screen.findByRole("alert")).toHaveTextContent("six digits");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("says an address it cannot read is not an address", async () => {
    render(<JoinHost onClose={() => undefined} onJoined={() => undefined} />);
    await userEvent.click(screen.getByRole("button", { name: "Join" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That is not an address");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("reports a host that is not there", async () => {
    fetchMock.mockRejectedValue(new Error("Failed to fetch"));
    render(<JoinHost onClose={() => undefined} onJoined={() => undefined} />);
    await fill("192.168.1.9:7878", "418902");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not reach http://192.168.1.9:7878",
    );
  });
});
