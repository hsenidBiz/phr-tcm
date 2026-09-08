import { History, X } from "lucide-react";
import { useState } from "react";
import type { PbiHit } from "../bindings";
import { loadRecents, removeRecent } from "./PbiPicker";

/** Recently-used PBIs for the pick-a-PBI empty states: a proper card grid
 * (the empty screens have the space), full titles, one click to scope -
 * and an X per row, because a finished PBI otherwise squats in the list
 * until enough newer ones push it out. */
export default function RecentPbis({
  org,
  project,
  onPick,
}: {
  org: string;
  project: string;
  onPick?: (pbi: PbiHit) => void;
}) {
  // Re-read per render; the bump only forces a render after a remove.
  // (Declared before the early returns - hooks cannot follow them.)
  const [, bump] = useState(0);
  if (!org || !project || !onPick) return null;
  const recents = loadRecents(org, project);
  if (recents.length === 0) return null;
  return (
    <div className="mt-6">
      <p className="mb-2 flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-faint">
        <History size={13} /> Recent PBIs
      </p>
      <div className="flex max-w-3xl flex-col gap-1.5">
        {recents.map((p) => (
          <div
            key={p.id}
            className="flex items-center gap-1 rounded-md border border-border bg-surface transition-colors hover:border-accent"
          >
            <button
              className="flex min-w-0 flex-1 items-baseline gap-2.5 rounded-md px-3 py-2 text-left transition-colors hover:bg-accent-soft"
              onClick={() => onPick(p)}
            >
              <span className="id-mono shrink-0 text-xs text-faint">#{p.id}</span>
              {/* Full title, wrapped - the empty state has the vertical room. */}
              <span className="min-w-0 break-words text-sm text-text">{p.title}</span>
            </button>
            <button
              aria-label={`Remove #${p.id} from recent PBIs`}
              title="Remove from recent PBIs"
              className="mr-2 shrink-0 rounded p-1 text-muted transition-colors hover:text-danger"
              onClick={() => {
                removeRecent(org, project, p.id);
                bump((v) => v + 1);
              }}
            >
              <X size={14} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
