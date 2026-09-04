// Settings → Phone & second screen: turn the LAN server on, show what a
// phone needs to join it, and list what has joined.
//
// The dialog is deliberately one screen with no steps. Everything a person
// needs in order to hand their phone the league is visible at once — the
// address, the QR of the same address, and the code — because the moment this
// gets used is the moment somebody is standing next to you holding a phone.

import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { useCompanion } from "../companion";
import type { CompanionDevice } from "../types";
import { age } from "../format";
import { useFocusTrap } from "./useFocusTrap";

import "../companion.css";

/** The sentence under the title. The whole of the security model, in one
 *  line, before anyone reads the code out loud. */
const GUIDANCE =
  "Same Wi-Fi only. Anyone with the code can read this league and ask questions on your budget.";

/** "418 902" — the six digits as they are read aloud. */
function spacedCode(code: string): string {
  return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

function DeviceRow({ device }: { device: CompanionDevice }) {
  return (
    <li className="companion-device">
      <span className={`device-glyph is-${device.kind}`} aria-hidden="true" />
      <span className="companion-device-name">{device.name}</span>
      <span className="muted small companion-device-kind">
        {device.kind === "phone" ? "Phone" : "Desktop"}
      </span>
      <span className={device.connected ? "companion-dot is-on" : "companion-dot"} />
      <span className="muted small companion-device-seen">
        {device.connected
          ? "Connected"
          : `Last seen ${age(Math.floor(device.last_seen_ms / 1000))}`}
      </span>
    </li>
  );
}

export function CompanionPanel({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const { status, devices, busy, error, enable, disable, revoke, rename } = useCompanion();
  // Kept with the URL it was drawn for, so a picture is never shown against
  // an address it does not encode — including the moment after the server is
  // switched off, when the effect below simply has nothing to draw.
  const [qr, setQr] = useState<{ url: string; image: string } | null>(null);
  const [tailQr, setTailQr] = useState<{ url: string; image: string } | null>(null);
  // What is in the name box while it is being edited; null while it is just
  // showing what the host is called.
  const [typedName, setTypedName] = useState<string | null>(null);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  // The QR is only ever of the URL on screen, so it is redrawn with it and
  // dropped the moment the server goes off — a stale code in a picture is the
  // one thing here nobody could debug by reading.
  const url = status?.enabled === true ? status.url : "";
  useEffect(() => {
    if (url === "") return undefined;
    let cancelled = false;
    void QRCode.toDataURL(url, { margin: 1, width: 168 })
      .then((image) => {
        if (!cancelled) setQr({ url, image });
      })
      .catch(() => {
        // The address is written out in full right beside it.
        if (!cancelled) setQr(null);
      });
    return () => {
      cancelled = true;
    };
  }, [url]);

  // Same again for the tailnet address, which a phone away from this Wi-Fi
  // scans instead. Absent unless the Mac is on Tailscale.
  const tailUrl = status?.enabled === true ? (status.tailscale_url ?? "") : "";
  useEffect(() => {
    if (tailUrl === "") return undefined;
    let cancelled = false;
    void QRCode.toDataURL(tailUrl, { margin: 1, width: 168 })
      .then((image) => {
        if (!cancelled) setTailQr({ url: tailUrl, image });
      })
      .catch(() => {
        if (!cancelled) setTailQr(null);
      });
    return () => {
      cancelled = true;
    };
  }, [tailUrl]);

  const on = status?.enabled ?? false;
  const hostName = status?.host_name ?? "";

  const commitName = () => {
    if (typedName === null) return;
    const next = typedName.trim();
    setTypedName(null);
    if (next !== "" && next !== hostName) rename(next);
  };

  return (
    <div
      className="scrim"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="dialog companion-dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="companion-title"
      >
        <span className="eyebrow">Settings</span>
        <span className="dialog-title" id="companion-title">
          Phone &amp; second screen
        </span>
        <span className="mid dialog-note">{GUIDANCE}</span>

        <div className="companion-toggle">
          <button
            type="button"
            className={on ? "btn-ghost" : "btn-primary"}
            disabled={busy || status === null}
            onClick={on ? disable : enable}
          >
            {on ? "Turn off" : "Turn on"}
          </button>
          <span className="muted small">
            {status === null
              ? "Asking this Mac…"
              : on
                ? `Listening on port ${status.port}`
                : "Off — nothing is being served"}
          </span>
        </div>

        {error !== null && (
          <div className="error" role="alert">
            {error}
          </div>
        )}

        {on && status !== null && (
          <div className="companion-join">
            <div className="companion-join-text">
              <span className="eyebrow">Open this on the phone</span>
              <span className="companion-url">{status.url}</span>
              {tailUrl !== "" && (
                <>
                  <span className="eyebrow">Or, over Tailscale, from anywhere</span>
                  <span className="companion-url">{tailUrl}</span>
                </>
              )}
              <span className="eyebrow companion-code-label">Then enter this code</span>
              <span className="companion-code">{spacedCode(status.code)}</span>
              <button type="button" className="btn-ghost btn-row" disabled={busy} onClick={revoke}>
                New code
              </button>
              <span className="muted small">A new code unpairs everything already connected.</span>
            </div>
            <div className="companion-qrs">
              {qr !== null && qr.url === status.url && (
                <img className="companion-qr" src={qr.image} alt={`QR code for ${status.url}`} />
              )}
              {tailQr !== null && tailQr.url === tailUrl && (
                <img className="companion-qr" src={tailQr.image} alt={`QR code for ${tailUrl}`} />
              )}
            </div>
          </div>
        )}

        <label className="field companion-name">
          Your name in shared chat
          <input
            className="text-input"
            value={typedName ?? hostName}
            placeholder="Justin's Mac"
            disabled={status === null}
            onChange={(e) => setTypedName(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitName();
              if (e.key === "Escape") setTypedName(null);
            }}
          />
        </label>

        <span className="eyebrow companion-devices-label">
          {devices.length === 0
            ? "No devices yet"
            : `${devices.length} device${devices.length === 1 ? "" : "s"}`}
        </span>
        {devices.length > 0 && (
          <ul className="companion-devices">
            {devices.map((device) => (
              <DeviceRow key={device.device_id} device={device} />
            ))}
          </ul>
        )}

        <div className="dialog-actions">
          <button type="button" className="btn-ghost" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
