// Auto Run's own execution order for a PBI. The list starts in the planned
// order (the saved one, else the suggested one) with a line between rows
// wherever the shared state has to be put back. Run Tests has its own
// order and this does not touch it.

import { useEffect, useId, useMemo, useState } from "react";
import { commands, type PlanView } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm, IconUndo } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "../ManageCases/CaseOrderList";
import { fetchPlan, resetLines } from "./plan";

export default function ExecutionOrderDialog({
  org,
  project,
  pbiId,
  cases,
  onClose,
}: {
  org: string;
  project: string;
  pbiId: number;
  /** The cases to order, in list order, with their titles. */
  cases: SuiteCase[];
  onClose: () => void;
}) {
  const titleId = useId();
  const titles = useMemo(() => new Map(cases.map((c) => [c.id, c.title])), [cases]);
  const titleOf = (id: number) => titles.get(id);
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [order, setOrder] = useState<SuiteCase[]>(cases);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let live = true;
    void fetchPlan(org, project, pbiId, cases.map((c) => c.id)).then((p) => {
      if (!live) return;
      setPlan(p);
      if (p) setOrder(p.order.map((id) => ({ id, title: titles.get(id) ?? "" })));
      setLoaded(true);
    });
    return () => {
      live = false;
    };
    // The dialog is mounted fresh on every open with the cases it was given.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const unchanged = plan != null && order.every((c, i) => c.id === plan.order[i]);
  // The reset points belong to the order the planner worked out. A list
  // that has been moved no longer matches them; they are worked out again
  // from the saved order.
  const showResets = plan != null && unchanged;

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try {
      await work();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  const save = () => run(() => unwrapStr(commands.autoRunSaveOrder(pbiId, order.map((c) => c.id))));
  const useSuggested = () => run(() => unwrapStr(commands.autoRunClearOrder(pbiId)));

  return (
    <Modal
      onClose={() => !busy && onClose()}
      labelledBy={titleId}
      className="flex max-h-[85vh] w-[640px] max-w-full flex-col gap-3 p-4"
    >
      <h2 id={titleId} className="text-sm font-semibold text-text">Execution order</h2>
      <p className="text-xs text-muted">
        The order Auto Run uses for this PBI on this machine. Run Tests keeps its own order.
      </p>
      {plan?.counts && (
        <p className="text-xs text-warning">
          {`this order needs ${plan.counts[0]} resets; the suggested order needs ${plan.counts[1]}`}
        </p>
      )}
      {!loaded && <p className="text-xs text-muted">Working out the order…</p>}
      {loaded && plan && !showResets && plan.resets.length > 0 && (
        <p className="text-xs text-muted">Reset points are worked out again once the order is saved.</p>
      )}
      {error && <p className="text-xs text-danger">{error}</p>}
      {loaded && (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <CaseOrderList
            cases={order}
            selected={selected}
            onChange={setOrder}
            onSelect={setSelected}
            ariaLabel="Auto Run execution order"
            disabled={busy}
            noteBefore={(id) => {
              if (!showResets) return null;
              const lines = plan.resets
                .filter((r) => r.before_case_id === id)
                .flatMap((r) => resetLines(r, titleOf));
              if (lines.length === 0) return null;
              return (
                <div className="space-y-0.5 border-l-2 border-l-warning bg-surface-2 px-3 py-1.5 text-xs text-warning">
                  {lines.map((l) => (
                    <p key={l}>{l}</p>
                  ))}
                </div>
              );
            }}
          />
        </div>
      )}
      <div className="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={busy} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        {plan?.saved && (
          <Button variant="outline" size="sm" disabled={busy} onClick={useSuggested}>
            <IconUndo aria-hidden />
            Use suggested order
          </Button>
        )}
        <Button size="sm" disabled={busy || !loaded || unchanged} onClick={save}>
          <IconConfirm aria-hidden />
          Save order
        </Button>
      </div>
    </Modal>
  );
}
