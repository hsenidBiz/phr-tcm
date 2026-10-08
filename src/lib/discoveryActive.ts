// Whether the assistant's discovery holds the Auto Run browser. While it
// does, Auto Run treats it as a run going: its Run buttons, Replay to step
// and Open browser wait, since each would take that browser away.
//
// Asked once (`auto_run_discovery_active`), then kept current by Rust's
// `autorun_discovery_changed`, which says when a discovery starts or ends.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { commands, events } from "../bindings";

/** Said by every control that waits for a discovery. */
export const DISCOVERY_BUSY = "Discovery is using the Auto Run browser";

const discoveryActiveKey = ["autorun-discovery-active"];

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
  return asked.data === true;
}
