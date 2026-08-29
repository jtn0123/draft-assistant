import { useEffect, useState } from "react";

/**
 * Milliseconds left until `deadline` (epoch ms), re-read every second, or
 * null when there is no deadline. Shared by the banner's clock and the
 * snake strip so the two never disagree by a tick.
 */
export function useCountdown(deadline: number | null): number | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (deadline === null) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [deadline]);
  return deadline === null ? null : deadline - now;
}
