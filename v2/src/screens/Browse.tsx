import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { Input } from "../components/ui/input";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import ExistingCases from "./ExistingCases";
import QueuePanel from "./QueuePanel";
import RunPanel from "./RunPanel";

/** PBI-centric hub: search a PBI, then everything (queue, linked cases,
 * runs) hangs off it. Org/project come from the context bar. */
export default function Browse({ org, project }: { org: string; project: string }) {
  const [searchText, setSearchText] = useState("");
  const [query, setQuery] = useState("");
  const [pbi, setPbi] = useState<PbiHit | null>(null);

  const pbis = useQuery({
    queryKey: ["pbis", org, project, query],
    queryFn: () => unwrap(commands.searchPbis(org, project, query)),
    enabled: Boolean(org && project && query),
    retry: false,
  });

  // Light summary fetch only to feed duplicate-title warnings in the queue;
  // the editor below does its own full fetch.
  const titles = useQuery({
    queryKey: ["pbi-tc-titles", org, pbi?.id],
    queryFn: () => unwrap(commands.pbiTestCases(org, pbi!.id)),
    enabled: Boolean(org && pbi),
    retry: false,
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to get started.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <label className="flex max-w-md flex-col gap-1 text-xs text-muted">
        Find PBI (Enter to search)
        <Input
          placeholder="Title or ID"
          value={searchText}
          onChange={(e) => setSearchText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              setQuery(searchText.trim());
              setPbi(null);
            }
          }}
        />
      </label>

      {pbis.isError && <p className="text-sm text-danger">{pbis.error.message}</p>}

      {pbis.data && (
        <ul className="space-y-1">
          {pbis.data.length === 0 && (
            <li className="text-sm text-muted">No PBIs match "{query}".</li>
          )}
          {pbis.data.map((hit) => (
            <li key={hit.id}>
              <button
                className={cn(
                  "w-full rounded-md border px-3 py-2 text-left text-sm transition-colors hover:border-accent",
                  pbi?.id === hit.id ? "border-accent bg-accent-soft" : "border-border",
                )}
                onClick={() => setPbi(hit)}
              >
                <span className="text-faint">#{hit.id}</span> {hit.title}
              </button>
            </li>
          ))}
        </ul>
      )}

      {pbi && (
        <QueuePanel
          org={org}
          project={project}
          pbiId={pbi.id}
          existingTitles={(titles.data ?? []).map((t) => t.title)}
        />
      )}

      {pbi && <ExistingCases org={org} project={project} pbiId={pbi.id} />}

      {pbi && <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />}
    </div>
  );
}
