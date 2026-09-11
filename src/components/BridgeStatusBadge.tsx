import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { cn } from "../lib/cn";
import { unwrapStr } from "../lib/ipc";

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
    <span
      role="status"
      title={
        up
          ? `Bridge listening on port ${bridge.data?.port}. It only runs while this app is open and signed in.`
          : "The bridge only runs while this app is open and signed in - AI tools can't reach it otherwise."
      }
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-[11px] font-medium",
        up ? "text-success" : "text-danger",
      )}
    >
      <span aria-hidden className="status-glow size-2 rounded-full bg-current" />
      {up ? "Running" : "Not running"}
    </span>
  );
}
