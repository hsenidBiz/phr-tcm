// An editor over one environment's Auto Run files closes, without saving,
// when the active environment changes: what it holds is the other
// environment's, and saving it would put it into this one.

import { useEffect, useRef, useSyncExternalStore } from "react";
import { activeEnvIdSnapshot, subscribeActiveEnv } from "../lib/activeEnv";
import { toast } from "../lib/toast";

/** Calls `onClose` once when the active environment changes from the one at
 * mount. Not when none was known at mount and the first one arrives: that
 * is the list loading, not a switch. */
export function useCloseOnEnvironmentChange(onClose: () => void): void {
  const id = useSyncExternalStore(subscribeActiveEnv, activeEnvIdSnapshot);
  const atMount = useRef(id);
  const closed = useRef(false);
  const latest = useRef(onClose);
  latest.current = onClose;
  useEffect(() => {
    if (closed.current || id === atMount.current) return;
    if (atMount.current === "") {
      atMount.current = id;
      return;
    }
    closed.current = true;
    toast.info("The environment changed, so this was closed without saving.");
    latest.current();
  }, [id]);
}
