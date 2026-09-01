// The one moment this app is allowed to interrupt: the clock reaching you.
//
// Synthesised rather than shipped as an asset, so there is no file to load,
// fail to load, or ship in the bundle. Its own module because App.tsx is the
// shell, not an audio engine, and because a chime nobody can hear is easier
// to notice in a test that imports it directly.

/** A short two-tone chime via WebAudio — no asset to ship or fail to load. */
export function playChime(): void {
  try {
    const Ctor = window.AudioContext ?? window.webkitAudioContext;
    if (Ctor === undefined) return;
    const ctx = new Ctor();
    const now = ctx.currentTime;
    for (const [i, freq] of [880, 1320].entries()) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = freq;
      osc.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + i * 0.16);
      gain.gain.exponentialRampToValueAtTime(0.18, now + i * 0.16 + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + i * 0.16 + 0.15);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + i * 0.16);
      osc.stop(now + i * 0.16 + 0.16);
    }
    window.setTimeout(() => void ctx.close(), 600);
  } catch {
    // An audio failure must never interrupt the draft.
  }
}
