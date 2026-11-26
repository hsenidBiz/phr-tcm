// What the assistants are doing, on every screen: a thin strip in the layout
// flow above the page (never over it), ONE short line, and an Open Auto Test
// button when the person is somewhere else. The per-assistant detail (and
// Stop) lives in Auto Test's Assistants card; hovering the line shows the
// detail. It draws nothing, and takes no height, while nothing is working.

import { useAgents } from "../hooks/useAgents";
import { activityText, isWorking } from "../lib/agentActivity";
import { IconGoTo } from "../lib/actionIcons";
import { Button } from "./ui/button";

/** The last part of a " / " path: the screen's own name. */
function lastSegment(path: string): string {
  const parts = path.split(" / ");
  return parts[parts.length - 1].trim();
}

export default function ActivityStrip({ onOpen, section }: { onOpen: () => void; section: string }) {
  const { agents, trial } = useAgents();
  const working = agents.filter((a) => isWorking(a.activity));
  if (working.length === 0 && !trial) return null;

  const discovering = working.filter((a) => a.activity.kind === "discovering");
  let line: string;
  if (discovering.length > 0) {
    const first = discovering[0].activity;
    const area = first.kind === "discovering" && first.area ? lastSegment(first.area) : null;
    const more = discovering.length > 1 ? ` (+${discovering.length - 1} more)` : "";
    line = `Discovery is running${area ? ` · ${area}` : ""}${more}`;
  } else if (working.length > 0) {
    line = working.length === 1 ? "An assistant is working" : `${working.length} assistants are working`;
  } else {
    line = `Trying a sign-in · ${lastSegment(trial?.environment ?? "")}`;
  }

  const detail = [
    ...working.map((a) =>
      [a.label, activityText(a.activity, a.idle_minutes), a.browser, a.account]
        .filter((p): p is string => Boolean(p))
        .join(" · "),
    ),
    ...(trial ? [`Trying a sign-in · ${trial.environment}`] : []),
  ].join("\n");

  return (
    <section aria-label="Assistant activity" className="border-b border-border bg-surface px-6 py-1.5 text-xs">
      <div className="flex items-center justify-between gap-3">
        <p className="min-w-0 truncate text-muted" title={detail}>
          {line}
        </p>
        {section !== "autorun" && (
          <Button size="sm" variant="outline" aria-label="Open Auto Test" className="shrink-0" onClick={onOpen}>
            <IconGoTo aria-hidden />
            Open Auto Test
          </Button>
        )}
      </div>
    </section>
  );
}
