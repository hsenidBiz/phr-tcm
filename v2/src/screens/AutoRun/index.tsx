// Supervised auto-run: the app drives a real Edge window through a
// case's steps while a person watches and decides the verdict.
//
// LOCAL ONLY. Nothing on this screen writes to Azure DevOps - the case
// list is read from it, and the results stay in this app until the
// feature has earned more trust than that.

import { ChevronDown, ChevronRight } from "lucide-react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { commands, type PbiHit } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Checkbox } from "../../components/ui/checkbox";
import { groupIndices } from "../../lib/grouping";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { Button } from "../../components/ui/button";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { IconEdit, IconImport } from "../../lib/actionIcons";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import PastRuns from "./PastRuns";
import RunPane from "./RunPane";
import ScriptEditor from "./ScriptEditor";

// How many imported case ids the success toast spells out before it falls
// back to a count - the same shape as the assigned-work notification
// summary. A 60-case import naming every one of them is unreadable.
const MAX_IDS_IN_TOAST = 10;

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
   * for the cases that just gained one.
   *
   * The path goes to Rust rather than reading the file here and sending
   * its contents: `readFileB64` + `atob` decodes to a latin-1 binary
   * string, so any non-ASCII byte (an accent, a curly quote) came out
   * mojibake, and a UTF-8 BOM made the JSON look corrupt before it ever
   * reached the parser. Rust reads the bytes itself now, the same way
   * the test-case JSON importer does. */
  const importScripts = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Action scripts", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.autoRunImportScripts(path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: async (ids) => {
      if (!ids) return;
      await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
      const shown = ids.slice(0, MAX_IDS_IN_TOAST);
      const rest = ids.length - shown.length;
      const caseList = `${shown.join(", ")}${rest > 0 ? `, and ${rest} more` : ""}`;
      toast.success(
        `Imported ${ids.length} script${ids.length === 1 ? "" : "s"} (case${
          ids.length === 1 ? "" : "s"
        } ${caseList}).`,
      );
    },
    // The generated `typedError` wrapper rethrows when the IPC call itself
    // rejects with an Error (rather than resolving to {status: "error"}) -
    // react-query's mutation still routes that here, same as an explicit
    // throw above, so this is the one place that needs to handle it.
    onError: (e) => toast.error(`Could not import that file: ${e.message}`),
  });
  /** The case ids queued for a run. `null` means no run is open. */
  const [running, setRunning] = useState<number[] | null>(null);
  /** Ticked cases, by id. A bulk run is these, in list order. */
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-autorun-group") === "on",
  );
  const [collapsed, toggleCollapsed] = usePersistedStringSet("tcm-v2-autorun-collapsed");

  const rows = cases.data ?? [];
  /** Same title-prefix grouping View Test Cases uses, so a person reading
   * both screens is reading one idea. */
  const groups = useMemo(
    () => (grouped ? groupIndices(rows.map((c) => c.title)) : []),
    [grouped, rows],
  );
  const hasScript = (i: number) => Boolean(scripts[i]?.data);
  /** Only scripted cases can be run, so only they can be ticked. */
  const runnableIn = (indices: number[]) =>
    indices.filter(hasScript).map((i) => rows[i].id);

  const toggleOne = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  /** A group heading ticks or clears every scripted case under it. The
   * "are they all on already" question is asked of `prev` inside the
   * updater, not of the `selected` this render closed over. */
  const toggleGroup = (indices: number[]) => {
    const ids = runnableIn(indices);
    setSelected((prev) => {
      const allOn = ids.length > 0 && ids.every((id) => prev.has(id));
      const next = new Set(prev);
      for (const id of ids) {
        if (allOn) next.delete(id);
        else next.add(id);
      }
      return next;
    });
  };

  /** List order, not click order - the run reads top to bottom the way
   * the screen does. */
  const selectedInOrder = rows.filter((c) => selected.has(c.id)).map((c) => c.id);

  /** One case row, by its index in `rows` - grouped and flat both render
   * the same thing, and `scripts[i]` is indexed the same way. */
  const row = (i: number) => {
    const c = rows[i];
    const ready = hasScript(i);
    return (
      <li
        key={c.id}
        className="flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"
      >
        {/* Only a scripted case can be run, so only a scripted case can be
            ticked - a checkbox that selects something unrunnable would
            just make the count lie. */}
        <Checkbox
          checked={selected.has(c.id)}
          ariaLabel={`Select #${c.id}`}
          className={ready ? undefined : "invisible"}
          onCheckedChange={() => ready && toggleOne(c.id)}
        />
        <span className="id-mono text-faint">#{c.id}</span>
        <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
        {ready ? (
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
        {ready && (
          <Button size="sm" aria-label={`Run #${c.id}`} onClick={() => setRunning([c.id])}>
            Run
          </Button>
        )}
      </li>
    );
  };

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  return (
    <div className="max-w-3xl space-y-4">
      <p className="rounded-md border border-accent/40 bg-accent-soft px-3 py-2 text-xs text-muted">
        Runs happen in a real browser window on this machine and you decide every verdict.
        Nothing is sent to Azure DevOps - results are saved here only.
      </p>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={importScripts.isPending}
          onClick={() => importScripts.mutate()}
        >
          <IconImport aria-hidden />
          Import scripts
        </Button>
        <span className="text-xs text-faint">
          One JSON file can carry every case in this PBI.
        </span>
        <label className="ml-auto flex cursor-pointer items-center gap-2 text-xs text-muted">
          <Checkbox
            checked={grouped}
            ariaLabel="Group by title"
            onCheckedChange={(on) => {
              setGrouped(on);
              try {
                localStorage.setItem("tcm-v2-autorun-group", on ? "on" : "off");
              } catch {
                // storage unavailable -> the choice lasts this session
              }
            }}
          />
          Group by title
        </label>
      </div>

      {cases.isLoading && <p className="text-sm text-muted">Loading test cases…</p>}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}

      {/* The bar only exists while something is ticked, so the screen is
          not carrying a permanently disabled button nobody can use. */}
      {selectedInOrder.length > 0 && (
        <div className="sticky top-0 z-10 flex items-center gap-2 rounded-md border border-accent/40 bg-accent-soft px-3 py-2">
          <span className="text-sm text-text">
            {selectedInOrder.length} case{selectedInOrder.length === 1 ? "" : "s"} selected
          </span>
          <Button size="sm" className="ml-auto" onClick={() => setRunning(selectedInOrder)}>
            Run {selectedInOrder.length} selected
          </Button>
          <Button size="sm" variant="outline" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
        </div>
      )}

      {grouped ? (
        groups.map(({ name, indices }) => {
          const label = name || "Ungrouped";
          const shut = collapsed.has(label);
          const ticked = indices.filter((i) => selected.has(rows[i].id)).length;
          return (
            <div key={label} className="space-y-1">
              <div className="flex w-full items-center gap-3 pb-1 pt-2">
                {/* Left-anchored with a trailing rule - see ViewCases for why. */}
                <button
                  aria-label={`${shut ? "Expand" : "Collapse"} group ${label}`}
                  title={shut ? "Expand group" : "Collapse group"}
                  className="text-muted transition-colors hover:text-accent"
                  onClick={() => toggleCollapsed(label)}
                >
                  {shut ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                </button>
                {/* Same contract as the other grouped screens: selection
                    is the checkbox's job (scripted cases only), the TITLE
                    toggles the fold like the chevron. */}
                {(() => {
                  const runnable = runnableIn(indices);
                  const on = runnable.filter((id) => selected.has(id)).length;
                  return (
                    <Checkbox
                      ariaLabel={`Select all in ${label}`}
                      checked={runnable.length > 0 && on === runnable.length}
                      indeterminate={on > 0 && on < runnable.length}
                      onCheckedChange={() => toggleGroup(indices)}
                    />
                  );
                })()}
                <button
                  className="group flex items-center gap-2"
                  title={shut ? "Expand group" : "Collapse group"}
                  onClick={() => toggleCollapsed(label)}
                >
                  <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                    {label} ({indices.length})
                  </span>
                </button>
                <span aria-hidden className="h-px flex-1 bg-border" />
              </div>
              {!shut && <ul className="space-y-1">{indices.map(row)}</ul>}
            </div>
          );
        })
      ) : (
        <ul className="space-y-1">{rows.map((_, i) => row(i))}</ul>
      )}

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
          const picked = running
            .map((id) => rows.find((x) => x.id === id))
            .filter((c): c is (typeof rows)[number] => Boolean(c))
            .map((c) => ({ id: c.id, title: c.title }));
          if (picked.length === 0) return null;
          return (
            <RunPane
              pbiId={pbi.id}
              cases={picked}
              onClose={() => {
                setRunning(null);
                // The selection has been run - leaving it ticked invites a
                // second run of cases that were just decided.
                setSelected(new Set());
              }}
            />
          );
        })()}
    </div>
  );
}
