// The host dialog, against the mocked backend the App tests share.

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { companionStatus, harness } from "../test/appHarness";
import type { CompanionDevice } from "../types";

vi.mock("../api", async () => ({ api: (await import("../test/appHarness")).harness().api }));
// jsdom has no canvas, so the picture is drawn by a stub; what matters here is
// that the dialog asks for one of the URL on screen and shows what comes back.
vi.mock("qrcode", () => ({
  default: { toDataURL: vi.fn(() => Promise.resolve("data:image/png;base64,QR")) },
}));

import QRCode from "qrcode";
import { CompanionPanel } from "./CompanionPanel";

const { api, push, reset } = harness();

function device(overrides: Partial<CompanionDevice> = {}): CompanionDevice {
  return {
    device_id: "d1",
    name: "Rob's iPhone",
    kind: "phone",
    paired_at_ms: Date.now() - 60000,
    last_seen_ms: Date.now() - 3000,
    connected: true,
    ...overrides,
  };
}

beforeEach(() => {
  reset();
  vi.mocked(QRCode.toDataURL).mockClear();
});

const open = () => render(<CompanionPanel onClose={() => undefined} />);

describe("turning it on", () => {
  it("starts off, and says what turning it on means", async () => {
    open();
    expect(await screen.findByRole("button", { name: "Turn on" })).toBeInTheDocument();
    expect(
      screen.getByText(
        /Same Wi-Fi only\. Anyone with the code can read this league and ask questions on your budget\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Off — nothing is being served")).toBeInTheDocument();
  });

  it("shows the address, the QR of it, and the code once it is on", async () => {
    api.companionEnable.mockResolvedValue(companionStatus({ enabled: true }));
    open();
    await userEvent.click(await screen.findByRole("button", { name: "Turn on" }));

    expect(await screen.findByText("http://192.168.1.24:7878/")).toBeInTheDocument();
    // Spaced for reading out, and still the six digits the host minted.
    expect(screen.getByText("418 902")).toBeInTheDocument();
    const qr = await screen.findByRole("img", { name: /QR code for http:\/\/192\.168\.1\.24/ });
    expect(qr).toHaveAttribute("src", "data:image/png;base64,QR");
    expect(QRCode.toDataURL).toHaveBeenCalledWith(
      "http://192.168.1.24:7878/",
      expect.objectContaining({ width: 168 }),
    );
    expect(screen.getByRole("button", { name: "Turn off" })).toBeInTheDocument();
  });

  it("adds the Tailscale address, with its own QR, when the Mac is on a tailnet", async () => {
    api.companionEnable.mockResolvedValue(
      companionStatus({ enabled: true, tailscale_url: "http://100.101.102.103:7878/" }),
    );
    open();
    await userEvent.click(await screen.findByRole("button", { name: "Turn on" }));

    expect(await screen.findByText("http://100.101.102.103:7878/")).toBeInTheDocument();
    expect(screen.getByText(/over Tailscale/)).toBeInTheDocument();
    // Both codes are on screen: the Wi-Fi one and the tailnet one.
    await screen.findByRole("img", { name: /QR code for http:\/\/100\.101\.102\.103/ });
    expect(screen.getAllByRole("img", { name: /QR code for/ })).toHaveLength(2);
  });

  it("shows no second address when there is no tailnet", async () => {
    api.companionEnable.mockResolvedValue(companionStatus({ enabled: true }));
    open();
    await userEvent.click(await screen.findByRole("button", { name: "Turn on" }));
    await screen.findByText("http://192.168.1.24:7878/");
    expect(screen.queryByText(/over Tailscale/)).not.toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: /QR code for/ })).toHaveLength(1);
  });
});

describe("the code", () => {
  it("mints a new one and warns what that costs", async () => {
    api.companionStatus.mockResolvedValue(companionStatus({ enabled: true }));
    api.companionRevoke.mockResolvedValue(companionStatus({ enabled: true, code: "654321" }));
    open();
    await userEvent.click(await screen.findByRole("button", { name: "New code" }));
    expect(await screen.findByText("654 321")).toBeInTheDocument();
    expect(api.companionRevoke).toHaveBeenCalledTimes(1);
    expect(
      screen.getByText("A new code unpairs everything already connected."),
    ).toBeInTheDocument();
  });
});

describe("this Mac's name", () => {
  it("shows the host name and saves an edited one", async () => {
    open();
    const box = await screen.findByLabelText("Your name in shared chat");
    expect(box).toHaveValue("Justin's Mac");
    await userEvent.clear(box);
    await userEvent.type(box, "Kitchen Mac{Enter}");
    expect(api.setDeviceName).toHaveBeenCalledWith("Kitchen Mac");
    await waitFor(() => expect(box).toHaveValue("Kitchen Mac"));
  });

  it("keeps the name it has when the box is emptied", async () => {
    open();
    const box = await screen.findByLabelText("Your name in shared chat");
    await userEvent.clear(box);
    await userEvent.tab();
    expect(api.setDeviceName).not.toHaveBeenCalled();
  });
});

describe("what has joined", () => {
  it("lists nothing until a device pairs, then follows the event", async () => {
    open();
    expect(await screen.findByText("No devices yet")).toBeInTheDocument();

    push.devices?.([device(), device({ device_id: "d2", name: "Justin's Mac", kind: "desktop" })]);

    expect(await screen.findByText("2 devices")).toBeInTheDocument();
    expect(screen.getByText("Rob's iPhone")).toBeInTheDocument();
    expect(screen.getByText("Phone")).toBeInTheDocument();
    expect(screen.getByText("Desktop")).toBeInTheDocument();
    expect(screen.getAllByText("Connected")).toHaveLength(2);
  });

  it("says when a device was last seen once it drops off", async () => {
    open();
    await screen.findByText("No devices yet");
    push.devices?.([device({ connected: false, last_seen_ms: Date.now() - 120000 })]);
    expect(await screen.findByText("Last seen 2m ago")).toBeInTheDocument();
  });
});
