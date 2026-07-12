import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { Input } from "../components/ui/input";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
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

  const testCases = useQuery({
    queryKey: ["pbi-tcs", org, pbi?.id],
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

      {pbi && <QueuePanel org={org} project={project} pbiId={pbi.id} />}

      {pbi && <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />}

      {pbi && (
        <section className="space-y-2">
          <h2 className="text-sm font-semibold text-muted">
            Test cases linked to #{pbi.id} {pbi.title}
          </h2>
          {testCases.isLoading && <p className="text-sm text-muted">Loading test cases...</p>}
          {testCases.isError && (
            <p className="text-sm text-danger">{testCases.error.message}</p>
          )}
          {testCases.data && testCases.data.length === 0 && (
            <p className="text-sm text-muted">No test cases linked yet.</p>
          )}
          {testCases.data && testCases.data.length > 0 && (
            <table className="w-full border-collapse text-sm">
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted">
                  <th className="px-2 py-1 font-medium">ID</th>
                  <th className="px-2 py-1 font-medium">Title</th>
                  <th className="px-2 py-1 font-medium">Tags</th>
                  <th className="px-2 py-1 font-medium">Automation</th>
                </tr>
              </thead>
              <tbody>
                {testCases.data.map((tc) => (
                  <tr key={tc.id} className="border-b border-border/50">
                    <td className="px-2 py-1 text-faint">{tc.id}</td>
                    <td className="px-2 py-1 text-text">{tc.title}</td>
                    <td className="px-2 py-1 text-muted">{tc.tags}</td>
                    <td className="px-2 py-1 text-muted">{tc.automation_status}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      )}
    </div>
  );
}
