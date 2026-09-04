// "Join another Draft Assistant…": point this app at somebody else's and run
// as a follower of it.
//
// Three fields, because there are exactly three unknowns — where the host is,
// the code on its screen, and what this machine should be called in the shared
// chat. The address box takes whatever the host showed: `192.168.1.5:7878`,
// the `http://…/` line, or the URL out of the QR, pasted whole.

import { useEffect, useRef, useState } from "react";
import { pairWithHost, parseHostAddress, reason, saveFollow } from "../companion";
import type { FollowRecord } from "../types";
import { useFocusTrap } from "./useFocusTrap";

import "../companion.css";

/** A name for this machine that is at least not blank. */
const DEFAULT_NICKNAME = "This Mac";

export function JoinHost({
  onClose,
  onJoined,
}: {
  onClose: () => void;
  /** What to do once the host has paired us. The default reloads, because
   *  every screen in the app has to be rebuilt against the host's data —
   *  patching a running window into follower mode would leave half of it
   *  pointed at the local backend. */
  onJoined?: (record: FollowRecord) => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const [nickname, setNickname] = useState(DEFAULT_NICKNAME);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  const join = async () => {
    const origin = parseHostAddress(address);
    if (origin === null) {
      setError("That is not an address. Try 192.168.1.5:7878, or paste the host's link.");
      return;
    }
    if (!/^\d{6}$/.test(code.trim())) {
      setError("The code is the six digits on the host's screen.");
      return;
    }
    setWorking(true);
    setError(null);
    try {
      const record = await pairWithHost(
        origin,
        code,
        nickname.trim() === "" ? DEFAULT_NICKNAME : nickname,
      );
      saveFollow(record);
      if (onJoined === undefined) window.location.reload();
      else onJoined(record);
    } catch (e) {
      setError(reason(e));
      setWorking(false);
    }
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
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="join-host-title"
      >
        <span className="eyebrow">Second screen</span>
        <span className="dialog-title" id="join-host-title">
          Join another Draft Assistant
        </span>
        <span className="join-host-note">
          Watch someone else&rsquo;s league on this Mac. They keep control of the league, the keys
          and the budget; you get the board, the season and the shared chat.
        </span>

        <div className="join-host-fields">
          <label className="field">
            Host address
            <input
              className="text-input"
              value={address}
              placeholder="192.168.1.5:7878"
              onChange={(e) => setAddress(e.target.value)}
            />
          </label>
          <label className="field">
            Code
            <input
              className="text-input"
              value={code}
              inputMode="numeric"
              placeholder="418902"
              onChange={(e) => setCode(e.target.value)}
            />
          </label>
          <label className="field">
            This device&rsquo;s name
            <input
              className="text-input"
              value={nickname}
              placeholder={DEFAULT_NICKNAME}
              onChange={(e) => setNickname(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void join();
              }}
            />
          </label>
        </div>

        {error !== null && (
          <span className="join-host-error" role="alert">
            {error}
          </span>
        )}

        <div className="dialog-actions">
          <button
            type="button"
            className="btn-primary"
            disabled={working}
            onClick={() => void join()}
          >
            {working ? "Joining…" : "Join"}
          </button>
          <button type="button" className="btn-ghost" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
