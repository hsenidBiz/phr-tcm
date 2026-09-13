import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconConfirm, IconImport, IconUndo } from "../../lib/actionIcons";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { orderFromFile, sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
// Explicit extension: this name differs from the component's own
// filename only by the case of its first letter ("suiteCases.ts" vs
// "SuiteCases.tsx"), and NTFS resolves filenames case-insensitively. An
// extensionless import here would be just as ambiguous as the
// component's own extensionless import from outside - see the
// `resolve.extensions` comment in vitest.config.ts and vite.config.ts.
import { loadSuiteCases, suiteCasesKey } from "./suiteCases.ts";

/** One expanded suite: its cases in Azure DevOps' order, re-orderable and
 * selectable. Order lives here (Apply saves it, Reset drops it); which
 * rows are selected lives in the screen, because a selection can span
 * the suites of a plan. */
export default function SuiteCases({
  org,
  project,
  planId,
  suiteId,
  selected,
  onToggle,
}: {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  /** Ids of this suite's cases the screen holds selected. */
  selected: Set<number>;
  /** Cases the user just checked (on) or unchecked (off). */
  onToggle: (cases: SuiteCase[], on: boolean) => void;
}) {
  const qc = useQueryClient();
  const key = suiteCasesKey(org, project, planId, suiteId);
  const cases = useQuery({
    queryKey: key,
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    retry: false,
  });

  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away. A fresh read replaces it.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  useEffect(() => {
    if (cases.data) setOrder(cases.data);
  }, [cases.data]);
  const dirty = cases.data ? !sameOrder(order, cases.data) : false;

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
      </div>
      <CaseOrderList
        cases={order}
        selected={selected}
        onChange={setOrder}
        onSelect={(next) => {
          const added = order.filter((c) => next.has(c.id) && !selected.has(c.id));
          const removed = order.filter((c) => !next.has(c.id) && selected.has(c.id));
          if (added.length) onToggle(added, true);
          if (removed.length) onToggle(removed, false);
        }}
        disabled={busy}
      />
    </div>
  );
}
