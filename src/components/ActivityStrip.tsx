// What the assistants are doing, on every screen: a thin strip in the layout
// flow above the page (never over it), one line per working assistant, with
// Stop where it can be stopped and one Open that goes to Auto Run. It draws
// nothing, and takes no height, while nothing is working.

import { useState } from "react";
import { commands } from "../bindings";
import { useAgents } from "../hooks/useAgents";
import { activityText, isWorking } from "../lib/agentActivity";
import { IconGoTo, IconStop } from "../lib/actionIcons";
import { toast } from "../lib/toast";
import { logUi } from "../lib/uiLog";
import { Button } from "./ui/button";

/** How many lines the strip draws before it says how many more there are. */
export const STRIP_MAX_LINES = 3;

export default function ActivityStrip({ onOpen }: { onOpen: () => void }) {
  const { agents, trial } = useAgents();
  // Slots whose Stop is in flight; declared above the early return below.
  const [stopping, setStopping] = useState<ReadonlySet<number>>(new Set());
  const working = agents.filter((a) => isWorking(a.activity));
  if (working.length === 0 && !trial) return null;

  const shown = working.slice(0, STRIP_MAX_LINES);
  const more = working.length - shown.length;

  async function stop(slot: number, label: string) {
    if (stopping.has(slot)) return;
    setStopping((s) => new Set(s).add(slot));
    try {
      const r = await commands.autoRunStopAgent(slot);
      if (r.status === "error") toast.error(r.error);
    } catch (e) {
      logUi(`activity strip: stop for ${label} failed: ${e instanceof Error ? e.message : String(e)}`);
      toast.error(`Could not stop ${label}. Try again, or see Settings → Logs.`);
    } finally {
      setStopping((s) => {
        const next = new Set(s);
        next.delete(slot);
        return next;
      });
    }
  }

  return (
    <section aria-label="Assistant activity" className="border-b border-border bg-surface px-6 py-1.5 text-xs">
      <div className="flex items-start justify-between gap-3">
        <ul className="min-w-0 space-y-0.5 text-muted">
          {shown.map((a) => {
            const slot = a.slot;
            const line = [a.label, activityText(a.activity, a.idle_minutes), a.browser, a.account]
              .filter((p): p is string => Boolean(p))
              .join(" · ");
            return (
              <li key={a.label} className="flex items-center gap-2">
                <span className="truncate">{line}</span>
                {a.can_stop && slot !== null && (
                  <Button
                    size="sm"
                    variant="outline"
                    aria-label={`Stop ${a.label}`}
                    aria-busy={stopping.has(slot)}
                    disabled={stopping.has(slot)}
                    onClick={() => void stop(slot, a.label)}
                  >
                    <IconStop aria-hidden />
                    Stop
                  </Button>
                )}
              </li>
            );
          })}
          {more > 0 && <li>{`and ${more} more`}</li>}
          {trial && <li>{`Trying a sign-in · ${trial.environment}`}</li>}
        </ul>
        <Button size="sm" variant="outline" aria-label="Open Auto Run" className="shrink-0" onClick={onOpen}>
          <IconGoTo aria-hidden />
          Open
        </Button>
      </div>
    </section>
  );
}
