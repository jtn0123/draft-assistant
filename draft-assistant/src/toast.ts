// The message strip's state: one message at a time, news that clears itself,
// and a failure that waits to be answered.
//
// It lives outside App because two of the shell's messages were being set
// straight into state, which skipped the timer and left them on screen for
// the rest of the session. One way in means one rule about how long a message
// stays.

import { useCallback, useEffect, useState } from "react";
import { REVOKED_KEY } from "./apiRemote";

/** A line under the header. `retry` marks it as something gone wrong that the
 *  user can have another go at — and that should wait for them. */
export interface ToastMessage {
  text: string;
  retry?: () => void;
}

export interface Toaster {
  toast: ToastMessage | null;
  showToast: (text: string, retry?: () => void) => void;
  dismissToast: () => void;
}

/** The pending "put this toast away" timer. Module scope rather than a ref:
 *  `showToast` is handed to the settings rows, and a function that reads a
 *  ref cannot be passed anywhere during render. One window, one toast strip,
 *  one timer — and the unmount effect still clears it. */
let toastTimer: number | undefined;

/** The note a revoked follower left for itself, taken and cleared. Null in
 *  every ordinary launch, which is all but one of them. */
function revokedNote(): ToastMessage | null {
  try {
    if (localStorage.getItem(REVOKED_KEY) === null) return null;
    localStorage.removeItem(REVOKED_KEY);
  } catch {
    return null;
  }
  return { text: "The host revoked this device" };
}

export function useToast(): Toaster {
  // Read as the state is built, so the shell paints with the explanation
  // rather than a frame after it, and read exactly once.
  const [note] = useState(revokedNote);
  const [toast, setToast] = useState<ToastMessage | null>(note);

  const showToast = useCallback((text: string, retry?: () => void) => {
    setToast({ text, retry });
    window.clearTimeout(toastTimer);
    // News gets out of the way on its own. Something that failed waits to be
    // answered — a lost pick in the middle of a draft is the worst thing this
    // app could shrug off.
    if (retry === undefined) {
      toastTimer = window.setTimeout(() => setToast(null), 5000);
    }
  }, []);

  const dismissToast = useCallback(() => {
    window.clearTimeout(toastTimer);
    setToast(null);
  }, []);

  // That note went up with no timer of its own and sat under the header for
  // the rest of the session. It is news like any other, so it gets the same
  // five seconds — and only if it is still the message on screen.
  useEffect(() => {
    if (note === null) return undefined;
    const timer = window.setTimeout(
      () => setToast((current) => (current === note ? null : current)),
      5000,
    );
    return () => window.clearTimeout(timer);
  }, [note]);

  useEffect(() => () => window.clearTimeout(toastTimer), []);

  return { toast, showToast, dismissToast };
}
