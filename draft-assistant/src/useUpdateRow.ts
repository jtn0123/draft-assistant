// The stateful half of Settings -> "Check for updates".
//
// One hook, one row's worth of state. The state machine itself is the pure
// module updateRow.ts; this owns the promise, the cancel flag and the React
// state around it, and hands the settings menu a `select` that does the right
// thing for wherever the row is.

import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import { useAppVersion } from "./appVersion";
import { failed, IDLE, selection, settled, started, type UpdateState } from "./updateRow";

export interface UpdateRowState {
  /** The running version, from the shell that knows it. */
  current: string;
  state: UpdateState;
  /** False where the shell has no updater: the browser preview, and a follower
   *  whose `api` speaks to somebody else's desktop. The menu leaves the row
   *  off then rather than offering a check that can only fail. */
  supported: boolean;
  /** What the row does when chosen: a check, an install, or nothing while one
   *  is already running. */
  select: () => void;
}

export function useUpdateRow(): UpdateRowState {
  const current = useAppVersion();
  const [state, setState] = useState<UpdateState>(IDLE);
  // A check that settles after the app moved on (a league switch remounts the
  // shell) must not write into a hook that is gone.
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const check = api.checkForUpdate;
  const install = api.installUpdate;
  const supported = check !== undefined && install !== undefined;

  // A plain closure over `state`, not a functional update: the work has to
  // happen once per selection, and React may run a state updater twice.
  const select = () => {
    if (check === undefined || install === undefined) return;
    const what = selection(state);
    if (what === "none") return;
    const settle = (next: UpdateState) => {
      if (alive.current) setState(next);
    };
    setState(started(state));
    if (what === "check") {
      check().then(
        (found) => settle(settled(found)),
        (error: unknown) => settle(failed(error)),
      );
    } else {
      // On success the app restarts and nothing here runs again; only the
      // failure path has a state to show.
      install().then(
        () => {},
        (error: unknown) => settle(failed(error)),
      );
    }
  };

  return { current, state, supported, select };
}
