// Whether the assistant's discovery holds the Auto Run browser. While it
// does, Auto Run treats it as a run going: its Run buttons, Replay to step
// and Open browser wait, since each would take that browser away.
//
// Asked once (`auto_run_discovery_active`), then kept current by Rust's
// `autorun_discovery_changed`, which says when a discovery starts or ends.
//
// What the hook last saw is also kept at module scope, for what is not a
// component or sits outside any query client: the unattended run's store
// (lib/backgroundRun) refuses to start a run on it, and the title-bar pill
// reads it. App's background-run host keeps the hook mounted, so it is
// current wherever the person is.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useSyncExternalStore } from "react";
import { commands, events } from "../bindings";

/** Said by every control that waits for a discovery. */
export const DISCOVERY_BUSY = "Discovery is using the Auto Run browser";

const discoveryActiveKey = ["autorun-discovery-active"];

let active = false;
const listeners = new Set<() => void>();

/** Whether a discovery holds the browser, as the hook last saw it. */
export function discoveryIsActive(): boolean {
  return active;
}

export function subscribeDiscoveryActive(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Records what the hook saw. Exported for tests. */
export function setDiscoveryActive(next: boolean): void {
  if (next === active) return;
  active = next;
  for (const l of listeners) l();
}

/** The module-scope value, for a component outside any query client. */
export function useDiscoveryActiveNow(): boolean {
  return useSyncExternalStore(subscribeDiscoveryActive, discoveryIsActive);
}

export function useDiscoveryActive(): boolean {
  const qc = useQueryClient();
  const asked = useQuery({
    queryKey: discoveryActiveKey,
    queryFn: async () => (await commands.autoRunDiscoveryActive()) === true,
    retry: false,
  });
  useEffect(() => {
    const un = events.autorunDiscoveryChanged.listen((e) => qc.setQueryData(discoveryActiveKey, e.payload.active));
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);
  const now = asked.data === true;
  useEffect(() => {
    setDiscoveryActive(now);
  }, [now]);
  return now;
}
