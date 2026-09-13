import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconConfirm, IconUndo } from "../../lib/actionIcons";
import { unwrap } from "../../lib/ipc";
import { sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

/** The suite's cases in Azure DevOps' own order. The entries carry the
 * order and the points carry the names; a case with several
 * configurations has several points and one row. */
export async function loadSuiteCases(
  org: string,
  project: string,
  planId: number,
  suiteId: number,
): Promise<SuiteCase[]> {
  const [entries, points] = await Promise.all([
    unwrap(commands.listSuiteEntries(org, project, suiteId)),
    unwrap(commands.listTestPoints(org, project, planId, suiteId)),
  ]);
  const names = new Map<number, string>();
  for (const p of points) {
    if (p.test_case_id != null && !names.has(p.test_case_id)) names.set(p.test_case_id, p.test_case_name);
  }
  return entries
    .filter((e) => e.entry_type === "testCase")
    .map((e) => ({ id: e.id, title: names.get(e.id) ?? `Test case ${e.id}` }));
}

/** Bulk operations on one suite's test cases: re-order them, move them to
 * another PBI, copy them into folders. The suite comes from the plan tree;
 * everything below the picker works on that one suite. */
export default function ManageCases({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [picked, setPicked] = useState<PickedSuite | null>(null);
  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const planId = picked?.planId ?? 0;
  const suiteId = picked?.suite.id ?? 0;
  const cases = useQuery({
    queryKey: ["suite-cases", org, project, planId, suiteId],
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    enabled: Boolean(picked),
    retry: false,
  });

  // A fresh read (new suite, or a reload after a save) replaces the
  // working order and clears the selection: rows may have gone.
  useEffect(() => {
    if (cases.data) {
      setOrder(cases.data);
      setSelected(new Set());
    }
  }, [cases.data]);

  const dirty = cases.data ? !sameOrder(order, cases.data) : false;

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: () => {
      toast.success("Order saved.");
      qc.invalidateQueries({ queryKey: ["suite-cases", org, project, planId, suiteId] });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  const busy = apply.isPending;

  return (
    <div className="space-y-4">
      <SuitePicker org={org} project={project} picked={picked} onPick={setPicked} />
      {!picked && (
        <p className="text-sm text-muted">
          Pick a test plan and a suite. Its test cases appear here in the order Azure DevOps shows them.
        </p>
      )}
      {picked && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
              <IconConfirm aria-hidden />
              {apply.isPending ? "Saving" : "Apply order"}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!dirty || busy}
              onClick={() => cases.data && setOrder(cases.data)}
            >
              <IconUndo aria-hidden />
              Reset
            </Button>
          </div>
          {cases.isLoading && <ScanProgress label="Loading test cases" />}
          {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
          {cases.data && cases.data.length === 0 && (
            <p className="text-sm text-muted">No test cases in this suite.</p>
          )}
          {cases.data && cases.data.length > 0 && (
            <CaseOrderList
              cases={order}
              selected={selected}
              onChange={setOrder}
              onSelect={setSelected}
              disabled={busy}
            />
          )}
        </>
      )}
    </div>
  );
}
