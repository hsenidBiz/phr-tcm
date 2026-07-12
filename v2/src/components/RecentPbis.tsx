import { History } from "lucide-react";
import type { PbiHit } from "../bindings";
import { loadRecents } from "./PbiPicker";

/** Recently-used PBIs for the pick-a-PBI empty states: a proper card grid
 * (the empty screens have the space), full titles, one click to scope. */
export default function RecentPbis({
  org,
  project,
  onPick,
}: {
  org: string;
  project: string;
  onPick?: (pbi: PbiHit) => void;
}) {
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
          <button
            key={p.id}
            title={p.title}
            className="flex items-baseline gap-2.5 rounded-md border border-border bg-surface px-3 py-2 text-left transition-colors hover:border-accent hover:bg-accent-soft"
            onClick={() => onPick(p)}
          >
            <span className="id-mono shrink-0 text-xs text-faint">#{p.id}</span>
            <span className="min-w-0 truncate text-sm text-text">{p.title}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
