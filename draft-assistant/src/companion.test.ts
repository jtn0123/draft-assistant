// Pairing with a host, and the identity this device keeps between pairings.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearFollow, pairWithHost, readFollow, saveFollow } from "./companion";

const fetchMock = vi.fn();

/** What `/api/pair` answers, as a `fetch` response. */
function paired(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as unknown as Response;
}

/** The body of the one request the pairing made. */
function sentBody(): Record<string, unknown> {
  const call = fetchMock.mock.calls[0] as [string, { body: string }] | undefined;
  if (call === undefined) throw new Error("nothing was sent");
  return JSON.parse(call[1].body) as Record<string, unknown>;
}

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  clearFollow();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("pairing", () => {
  it("keeps the id the host gave this device", async () => {
    // Without this the follow record had no identity at all: the id came back
    // on the pair response and was thrown away.
    fetchMock.mockResolvedValue(
      paired({ token: "t1", host_name: "Justin's Mac", device_id: "d1" }),
    );
    const record = await pairWithHost("http://192.168.1.5:7878", "418902", "This Mac");
    expect(record).toMatchObject({ token: "t1", host_name: "Justin's Mac", device_id: "d1" });
  });

  it("sends nothing about a previous device on a first pairing", async () => {
    fetchMock.mockResolvedValue(paired({ token: "t1", host_name: "Host" }));
    await pairWithHost("http://192.168.1.5:7878", "418902", "This Mac");
    expect(sentBody()).toEqual({ code: "418902", device_name: "This Mac", kind: "desktop" });
  });

  it("names the last pairing so the host replaces it instead of listing it twice", async () => {
    // The failure: re-pairing after a token expiry left the host's device list
    // showing "This Mac" and "This Mac 2" for one machine, and revoking either
    // one of them did nothing useful.
    fetchMock.mockResolvedValue(paired({ token: "t2", host_name: "Host", device_id: "d1" }));
    await pairWithHost("http://192.168.1.5:7878", "418902", "This Mac", "desktop", "d1");
    expect(sentBody().device_id).toBe("d1");
  });

  it("survives a record written before ids were kept", async () => {
    saveFollow({ url: "http://h:7878", token: "old", host_name: "Host" });
    expect(readFollow()?.device_id).toBeUndefined();

    fetchMock.mockResolvedValue(paired({ token: "t3", host_name: "Host", device_id: "d9" }));
    await pairWithHost("http://h:7878", "418902", "This Mac", "desktop", readFollow()?.device_id);
    expect(sentBody().device_id).toBeUndefined();
  });
});
