import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { unwrapStr } from "../lib/ipc";
import StatusPill from "./StatusPill";

/** The bridge's state as a glowing dot beside the AI Bridge title: green
 * while it listens, red when it does not. Replaces a whole "Status" card
 * that said the same thing in a sentence. Shares the tab's query key, so
 * the tab and the badge never disagree. */
export default function BridgeStatusBadge() {
  const bridge = useQuery({
    queryKey: ["bridge-status"],
    queryFn: () => unwrapStr(commands.bridgeStatus()),
    retry: false,
  });
  if (bridge.isPending) return null;
  const up = Boolean(bridge.data);
  return (
    <StatusPill
      tone={up ? "success" : "danger"}
      label={up ? "Running" : "Not running"}
      title={
        up
          ? `Bridge listening on port ${bridge.data?.port}. It only runs while this app is open and signed in.`
          : "The bridge only runs while this app is open and signed in - AI tools can't reach it otherwise."
      }
    />
  );
}
