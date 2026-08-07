// Supervised auto-run: the app drives a real Edge window through a
// case's steps while a person watches and decides the verdict.
//
// LOCAL ONLY. Nothing on this screen writes to Azure DevOps - the case
// list is read from it, and the results stay in this app until the
// feature has earned more trust than that.

import { useQueries, useQuery } from "@tanstack/react-query";
import { commands, type PbiHit } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrap, unwrapStr } from "../../lib/ipc";

export default function AutoRun({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { prefs } = useFieldRefs(org, project);

  const cases = useQuery({
    queryKey: ["autorun-cases", org, pbi?.id, prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(
        commands.pbiTestCasesFull(org, pbi!.id, prefs.moduleRef, prefs.preconditionsRef),
      ),
    enabled: Boolean(org && pbi),
    retry: false,
  });

  // One script lookup per case, so the list can say which are drivable.
  const scripts = useQueries({
    queries: (cases.data ?? []).map((c) => ({
      queryKey: ["autorun-script", c.id],
      queryFn: () => unwrapStr(commands.autoRunLoadScript(c.id)),
      retry: false,
    })),
  });

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  return (
    <div className="max-w-3xl space-y-4">
      <p className="rounded-md border border-accent/40 bg-accent-soft px-3 py-2 text-xs text-muted">
        Runs happen in a real Edge window on this machine and you decide every verdict.
        Nothing is sent to Azure DevOps - results are saved here only.
      </p>

      {cases.isLoading && <p className="text-sm text-muted">Loading test cases…</p>}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}

      <ul className="space-y-1">
        {(cases.data ?? []).map((c, i) => (
          <li
            key={c.id}
            className="flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"
          >
            <span className="id-mono text-faint">#{c.id}</span>
            <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
            {scripts[i]?.data ? (
              <Badge className="bg-success/15 text-success">Script ready</Badge>
            ) : (
              <Badge className="bg-surface-2 text-faint">No script</Badge>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
