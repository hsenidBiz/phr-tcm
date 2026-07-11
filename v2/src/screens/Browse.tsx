import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { unwrap } from "../lib/ipc";
import QueuePanel from "./QueuePanel";
import RunPanel from "./RunPanel";

const inputCls =
  "rounded-md border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none";

export default function Browse({
  org,
  setOrg,
  project,
  setProject,
}: {
  org: string;
  setOrg: (v: string) => void;
  project: string;
  setProject: (v: string) => void;
}) {
  const [searchText, setSearchText] = useState("");
  const [query, setQuery] = useState("");
  const [pbi, setPbi] = useState<PbiHit | null>(null);

  const orgs = useQuery({
    queryKey: ["orgs"],
    queryFn: () => unwrap(commands.listOrgs()),
  });

  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: () => unwrap(commands.listProjects(org)),
    enabled: Boolean(org),
  });

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

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-3">
        <label className="flex flex-col gap-1 text-xs text-neutral-400">
          Organization
          <select
            className={inputCls + " w-56"}
            value={org}
            onChange={(e) => {
              setOrg(e.target.value);
              setProject("");
              setPbi(null);
            }}
          >
            <option value="">Select organization...</option>
            {(orgs.data ?? []).map((o) => (
              <option key={o.name} value={o.name}>
                {o.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-neutral-400">
          Project
          <select
            className={inputCls + " w-56"}
            value={project}
            disabled={!org}
            onChange={(e) => {
              setProject(e.target.value);
              setPbi(null);
            }}
          >
            <option value="">Select project...</option>
            {(projects.data ?? []).map((p) => (
              <option key={p.id} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-neutral-400">
          Find PBI (Enter to search)
          <input
            className={inputCls + " w-72"}
            placeholder="Title or ID"
            value={searchText}
            disabled={!project}
            onChange={(e) => setSearchText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") setQuery(searchText.trim());
            }}
          />
        </label>
      </div>

      {orgs.isLoading && <p className="text-sm text-neutral-400">Loading organizations...</p>}
      {orgs.isError && <p className="text-sm text-red-400">{orgs.error.message}</p>}
      {pbis.isError && <p className="text-sm text-red-400">{pbis.error.message}</p>}

      {pbis.data && (
        <ul className="space-y-1">
          {pbis.data.length === 0 && (
            <li className="text-sm text-neutral-400">No PBIs match "{query}".</li>
          )}
          {pbis.data.map((hit) => (
            <li key={hit.id}>
              <button
                className={
                  "w-full rounded-md border px-3 py-2 text-left text-sm hover:border-blue-500 " +
                  (pbi?.id === hit.id ? "border-blue-500 bg-neutral-900" : "border-neutral-800")
                }
                onClick={() => setPbi(hit)}
              >
                <span className="text-neutral-500">#{hit.id}</span> {hit.title}
              </button>
            </li>
          ))}
        </ul>
      )}

      {pbi && <QueuePanel org={org} project={project} pbiId={pbi.id} />}

      {pbi && <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />}

      {pbi && (
        <section className="space-y-2">
          <h2 className="text-sm font-semibold text-neutral-300">
            Test cases linked to #{pbi.id} {pbi.title}
          </h2>
          {testCases.isLoading && <p className="text-sm text-neutral-400">Loading test cases...</p>}
          {testCases.isError && (
            <p className="text-sm text-red-400">{testCases.error.message}</p>
          )}
          {testCases.data && testCases.data.length === 0 && (
            <p className="text-sm text-neutral-400">No test cases linked yet.</p>
          )}
          {testCases.data && testCases.data.length > 0 && (
            <table className="w-full border-collapse text-sm">
              <thead>
                <tr className="border-b border-neutral-800 text-left text-xs text-neutral-400">
                  <th className="px-2 py-1 font-medium">ID</th>
                  <th className="px-2 py-1 font-medium">Title</th>
                  <th className="px-2 py-1 font-medium">Tags</th>
                  <th className="px-2 py-1 font-medium">Automation</th>
                </tr>
              </thead>
              <tbody>
                {testCases.data.map((tc) => (
                  <tr key={tc.id} className="border-b border-neutral-900">
                    <td className="px-2 py-1 text-neutral-500">{tc.id}</td>
                    <td className="px-2 py-1">{tc.title}</td>
                    <td className="px-2 py-1 text-neutral-400">{tc.tags}</td>
                    <td className="px-2 py-1 text-neutral-400">{tc.automation_status}</td>
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
