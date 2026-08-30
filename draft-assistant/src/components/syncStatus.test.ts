import { describe, expect, it } from "vitest";
import { quietSecs, syncClass, syncLabel } from "./syncStatus";

const NOW = 1_800_000_000_000;
const health = (secondsAgo: number, failures = 0) => ({
  last_success_at: Math.floor(NOW / 1000) - secondsAgo,
  consecutive_failures: failures,
  last_error: null,
});

describe("sync badge", () => {
  it("waits for the beat the backend actually keeps", () => {
    // The draft polls every 3s; the season loop sleeps 60s between polls
    // (`app_season::SEASON_IDLE`), so 30s of quiet is a working feed.
    expect(quietSecs(false)).toBe(30);
    expect(quietSecs(true)).toBe(180);
    // A slower draft beat pushes the threshold out with it.
    expect(quietSecs(false, 20)).toBe(60);
  });

  it("stays green through a season poll gap that used to turn it red", () => {
    // Watched live: the pill read "Sync stale · nothing for 45s" on a feed
    // that was perfectly healthy, every minute, all season.
    expect(syncLabel(true, health(45), NOW, true)).toBe("● Live sync on");
    expect(syncClass(true, health(45), NOW, true)).toBe("on");
    // Mid-draft, 45s of silence is still news.
    expect(syncLabel(true, health(45), NOW, false)).toBe("● Sync stale · nothing for 45s");
  });

  it("still calls a season feed that has really stopped", () => {
    expect(syncClass(true, health(400), NOW, true)).toBe("stale");
    expect(syncLabel(true, health(400), NOW, true)).toBe("● Sync stale · nothing for 6m");
  });

  it("reports failures before it reports silence, and says nothing when off", () => {
    expect(syncLabel(true, health(1, 2), NOW, true)).toBe("● Sync stale · 2 failures");
    expect(syncClass(true, health(1, 1), NOW, true)).toBe("retrying");
    expect(syncLabel(false, health(1), NOW, true)).toBe("○ Live sync off");
    expect(syncClass(false, health(999), NOW, true)).toBe("");
  });
});
