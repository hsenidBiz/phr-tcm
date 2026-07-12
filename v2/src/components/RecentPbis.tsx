import type { PbiHit } from "../bindings";
import { loadRecents } from "./PbiPicker";

/** Chip row of recently-used PBIs for the pick-a-PBI empty states. */
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
    <div className="mt-3 flex flex-wrap items-center gap-1.5">
      <span className="text-xs text-faint">Recent:</span>
      {recents.map((p) => (
        <button
          key={p.id}
          className="rounded-full border border-border px-2.5 py-0.5 text-xs text-muted transition-colors hover:border-accent hover:text-accent"
          onClick={() => onPick(p)}
        >
          <span className="id-mono">#{p.id}</span>{" "}
          {p.title.length > 45 ? `${p.title.slice(0, 45)}…` : p.title}
        </button>
      ))}
    </div>
  );
}
