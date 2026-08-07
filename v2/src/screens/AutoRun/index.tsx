// Supervised auto-run: the app drives a real Edge window through a
// case's steps while a person watches and decides the verdict.
//
// LOCAL ONLY. Nothing on this screen writes to Azure DevOps - the case
// list is read from it, and the results stay in this app until the
// feature has earned more trust than that.

import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type PbiHit } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { IconEdit, IconImport } from "../../lib/actionIcons";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import PastRuns from "./PastRuns";
import RunPane from "./RunPane";
import ScriptEditor from "./ScriptEditor";

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

  const [editing, setEditing] = useState<number | null>(null);
  const queryClient = useQueryClient();

  /** One file, many cases - the shape `save_autorun_script` writes, so an
   * assistant's whole-PBI output imports in one go. Every badge is
   * invalidated afterwards, or the rows would keep saying "No script"
   * for the cases that just gained one. */
  const importScripts = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "Action scripts", extensions: ["json"] }],
    }).catch(() => null);
    if (typeof path !== "string") return;
    const text = await commands.readFileB64(path);
    if (text.status === "error") {
      toast.error(`Could not read that file: ${text.error}`);
      return;
    }
    const json = atob(text.data.b64);
    const r = await commands.autoRunImportScripts(json);
    if (r.status === "error") {
      toast.error(r.error);
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
    toast.success(
      `Imported ${r.data.length} script${r.data.length === 1 ? "" : "s"} (case${
        r.data.length === 1 ? "" : "s"
      } ${r.data.join(", ")}).`,
    );
  };
  const [running, setRunning] = useState<number | null>(null);

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  return (
    <div className="max-w-3xl space-y-4">
      <p className="rounded-md border border-accent/40 bg-accent-soft px-3 py-2 text-xs text-muted">
        Runs happen in a real Edge window on this machine and you decide every verdict.
        Nothing is sent to Azure DevOps - results are saved here only.
      </p>

      <div className="flex items-center gap-2">
        <Button size="sm" variant="outline" onClick={importScripts}>
          <IconImport aria-hidden />
          Import scripts
        </Button>
        <span className="text-xs text-faint">
          One JSON file can carry every case in this PBI.
        </span>
      </div>

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
            <Button
              size="sm"
              variant="outline"
              aria-label={`Edit script for #${c.id}`}
              onClick={() => setEditing(c.id)}
            >
              <IconEdit aria-hidden />
              Script
            </Button>
            {scripts[i]?.data && (
              <Button
                size="sm"
                aria-label={`Run #${c.id}`}
                onClick={() => setRunning(c.id)}
              >
                Run
              </Button>
            )}
          </li>
        ))}
      </ul>

      <PastRuns />

      {editing != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === editing);
          if (!c) return null;
          return (
            <ScriptEditor
              caseId={c.id}
              title={c.title}
              steps={c.steps}
              onClose={() => setEditing(null)}
            />
          );
        })()}

      {running != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === running);
          if (!c) return null;
          return (
            <RunPane
              pbiId={pbi.id}
              caseId={c.id}
              title={c.title}
              onClose={() => setRunning(null)}
            />
          );
        })()}
    </div>
  );
}
