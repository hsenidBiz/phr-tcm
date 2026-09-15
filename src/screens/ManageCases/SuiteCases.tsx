import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { Switch } from "../../components/ui/switch";
import { IconConfirm, IconImport, IconUndo } from "../../lib/actionIcons";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { orderByGroups, orderFromFile, orderGroupsAZ, sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
import { markSuiteDirty, useDirtyRank } from "./dirtySuites";
import { loadSuiteCases, suiteCasesKey } from "./suiteCasesQuery";

/** One expanded suite: its cases in Azure DevOps' order, re-orderable and
 * selectable. Order lives here (Apply saves it, Reset drops it); which
 * rows are selected lives in the screen, because a selection can span
 * the suites of a plan. */
export default function SuiteCases({
  org,
  project,
  planId,
  suiteId,
  suiteName,
  selected,
  onToggle,
}: {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  /** For the case list's accessible name: several lists can be open at once. */
  suiteName: string;
  /** Ids of this suite's cases the screen holds selected. */
  selected: Set<number>;
  /** Cases the user just checked (on) or unchecked (off). */
  onToggle: (cases: SuiteCase[], on: boolean) => void;
}) {
  const qc = useQueryClient();
  const key = suiteCasesKey(org, project, planId, suiteId);
  // Reopening a suite must not re-read it: two Azure DevOps calls per
  // suite made Suite Management feel like it was loading from scratch
  // every time, even standing still on one PBI. Served from disk at once,
  // refreshed in the background after five minutes - and this screen's own
  // edits (reorder, copy in, new suite) invalidate the key, so the user's
  // own changes never wait for that.
  const cases = useQuery({
    queryKey: key,
    ...persistentQuery({
      key: cacheKeys.suiteCases(org, project, planId, suiteId),
      fetcher: () => loadSuiteCases(org, project, planId, suiteId),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    retry: false,
  });

  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away. A fresh read replaces it.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  // Remembered app-wide like the other screens' group switches. Turning
  // it ON arranges the list (a real reorder - Apply order lights up);
  // OFF only hides the headers. A suite opened with it already on is NOT
  // rearranged: nothing the user did not ask for may make the list dirty.
  const [grouped, setGrouped] = useState(() => {
    try {
      return localStorage.getItem("tcm-v2-group-manage") === "on";
    } catch {
      return false;
    }
  });
  const setGroupedAndRemember = (on: boolean) => {
    setGrouped(on);
    try {
      localStorage.setItem("tcm-v2-group-manage", on ? "on" : "off");
    } catch {
      // session-only
    }
    if (on) setOrder((o) => orderByGroups(o));
  };
  useEffect(() => {
    if (cases.data) setOrder(cases.data);
  }, [cases.data]);
  const dirty = cases.data ? !sameOrder(order, cases.data) : false;
  const rank = useDirtyRank(suiteId);
  useEffect(() => {
    markSuiteDirty(suiteId, dirty);
  }, [suiteId, dirty]);
  useEffect(() => () => markSuiteDirty(suiteId, false), [suiteId]);

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: (serverIds) => {
      toast.success("Order saved.");
      // Write the server's own order into the cache first so `dirty`
      // reads false at once, instead of Apply flipping back on for the
      // beat before the refetch lands.
      const byId = new Map(order.map((c) => [c.id, c]));
      const next = serverIds.map((id) => byId.get(id)).filter((c): c is SuiteCase => c != null);
      qc.setQueryData(key, next);
      qc.invalidateQueries({ queryKey: key });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  /** A draft .json carries each uploaded case's id and, after the
   * optimizer's grouping pass, its tester_order. The file only proposes:
   * the list re-orders on screen and Apply order is what saves it. */
  const fromFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const parsed = await unwrapStr(commands.parseImportFile(path));
      return orderFromFile(order, parsed.cases);
    },
    onSuccess: (result) => {
      if (!result) return;
      if (result.matched === 0) {
        toast.warning("No test case in that file is in this suite. The file needs ids from an upload.");
        return;
      }
      setOrder(result.order);
      toast.info(`Placed ${result.matched} of ${order.length} test cases from the file. Apply order to save.`);
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });

  const busy = apply.isPending || fromFile.isPending;

  if (cases.isLoading) return <ScanProgress label="Loading test cases" className="my-2" />;
  if (cases.isError) return <p className="my-2 text-sm text-danger">{cases.error.message}</p>;
  if (!cases.data || cases.data.length === 0)
    return <p className="my-2 text-sm text-muted">No test cases in this suite.</p>;

  return (
    <div className="my-2 space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
          <IconConfirm aria-hidden />
          {apply.isPending ? "Saving" : "Apply order"}
        </Button>
        <Button size="sm" variant="ghost" disabled={!dirty || busy} onClick={() => cases.data && setOrder(cases.data)}>
          <IconUndo aria-hidden />
          Reset
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => fromFile.mutate()}>
          <IconImport aria-hidden />
          {fromFile.isPending ? "Reading file" : "Apply tester order from file"}
        </Button>
        <label className="ml-2 flex items-center gap-2 text-xs text-muted">
          <Switch checked={grouped} onCheckedChange={setGroupedAndRemember} ariaLabel="Group by title" />
          Group by title
        </label>
        {grouped && (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            title="Every group together, groups A to Z"
            onClick={() => setOrder((o) => orderGroupsAZ(o))}
          >
            A-Z groups
          </Button>
        )}
      </div>
      <CaseOrderList
        cases={order}
        selected={selected}
        ariaLabel={`Test cases in ${suiteName}`}
        onChange={setOrder}
        onSelect={(next) => {
          const added = order.filter((c) => next.has(c.id) && !selected.has(c.id));
          const removed = order.filter((c) => !next.has(c.id) && selected.has(c.id));
          if (added.length) onToggle(added, true);
          if (removed.length) onToggle(removed, false);
        }}
        disabled={busy}
        grouped={grouped}
      />
      {rank != null && (
        // Same sticky treatment as Run Tests' Close all: a long suite puts
        // Apply order a screen and a half above the row being dragged.
        // Stacked by rank, because several suites can be open and unsaved.
        <div
          role="region"
          aria-label={`Unsaved order in ${suiteName}`}
          className="fixed right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface p-2.5 shadow-2xl"
          style={{ bottom: `${1.5 + rank * 3.5}rem` }}
        >
          <span className="max-w-48 truncate pl-1 text-xs text-muted">{suiteName}</span>
          <Button size="sm" disabled={busy} onClick={() => apply.mutate()}>
            <IconConfirm aria-hidden />
            {apply.isPending ? "Saving" : "Apply order"}
          </Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => cases.data && setOrder(cases.data)}>
            <IconUndo aria-hidden />
            Reset
          </Button>
        </div>
      )}
    </div>
  );
}
