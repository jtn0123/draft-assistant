const KEY = "draft-assistant.on-clock-alert";

/** Chime when your pick comes up. On by default: missing your pick costs more
 *  than a noise you did not want. */
export function loadAlertPref(): boolean {
  try {
    return window.localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function saveAlertPref(on: boolean): void {
  try {
    window.localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // Storage unavailable; the choice still applies for this session.
  }
}
