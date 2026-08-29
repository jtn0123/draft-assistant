import { useEffect, useRef } from "react";

const IDLE_TITLE = "Draft Assistant";

/** A short two-tone chime, synthesised so there is no asset to ship or load. */
function chime() {
  type WebAudioWindow = Window & { webkitAudioContext?: typeof AudioContext };
  const Ctor = window.AudioContext ?? (window as WebAudioWindow).webkitAudioContext;
  if (!Ctor) return;
  const ctx = new Ctor();
  const play = (at: number, hz: number) => {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.frequency.value = hz;
    osc.type = "sine";
    // Ramped, not switched: a square edge on a gain node clicks.
    gain.gain.setValueAtTime(0.0001, ctx.currentTime + at);
    gain.gain.exponentialRampToValueAtTime(0.25, ctx.currentTime + at + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + at + 0.35);
    osc.connect(gain).connect(ctx.destination);
    osc.start(ctx.currentTime + at);
    osc.stop(ctx.currentTime + at + 0.4);
  };
  play(0, 880);
  play(0.18, 1320);
  window.setTimeout(() => void ctx.close(), 1200);
}

/**
 * Say something when your pick comes up, for the times you are looking at
 * Sleeper in another window rather than at this one: a chime, and the
 * window title so the Dock and ⌘-Tab carry it too.
 *
 * Fires on the transition into your pick, once per pick — not on every
 * poll, which lands every three seconds and would be unbearable.
 */
export function useOnClockAlert({
  onClock,
  currentPick,
  enabled,
}: {
  onClock: boolean;
  currentPick?: number;
  enabled: boolean;
}) {
  const alerted = useRef<number | null>(null);

  useEffect(() => {
    if (!onClock || currentPick === undefined) {
      document.title = IDLE_TITLE;
      return;
    }
    document.title = `⏰ YOUR PICK ${currentPick} — ${IDLE_TITLE}`;
    if (!enabled || alerted.current === currentPick) return;
    alerted.current = currentPick;
    try {
      chime();
    } catch {
      // No audio device, or the webview refused: the title still changed.
    }
  }, [onClock, currentPick, enabled]);

  // Never leave the title shouting once the panel is gone.
  useEffect(() => () => void (document.title = IDLE_TITLE), []);
}
