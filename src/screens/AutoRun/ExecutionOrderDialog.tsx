// Auto Run's own execution order for a PBI. The list starts in the planned
// order (the saved one, else the suggested one) with a line between rows
// wherever the shared state has to be put back. Run Tests has its own
// order and this does not touch it.

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { commands, type PlanView } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import { IconCancel, IconConfirm, IconUndo } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { logUi } from "../../lib/uiLog";
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
  /** The first plan's order, and whether an order of its own was saved: the
   * baseline Save and Use suggested order are judged against. */
  const [baseline, setBaseline] = useState<number[] | null>(null);
  const [savedOnDisk, setSavedOnDisk] = useState(false);
  const [failed, setFailed] = useState(false);
  /** Counts the questions asked, so an old answer cannot overwrite a newer one. */
  const asked = useRef(0);
  const [order, setOrder] = useState<SuiteCase[]>(cases);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let live = true;
    const mine = ++asked.current;
    void fetchPlan(org, project, pbiId, cases.map((c) => c.id)).catch(() => null).then((p) => {
      if (!live || mine !== asked.current) return;
      setPlan(p);
      setFailed(p == null);
      if (p) {
        setOrder(p.order.map((id) => ({ id, title: titles.get(id) ?? "" })));
        setBaseline(p.order);
        setSavedOnDisk(p.saved);
      }
      setLoaded(true);
    });
    return () => {
      live = false;
    };
    // The dialog is mounted fresh on every open with the cases it was given.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const unchanged = baseline != null && order.every((c, i) => c.id === baseline[i]);

  /** The list was moved: work out its reset points again. The old lines
   * stay on screen until the answer arrives. */
  const moved = (next: SuiteCase[]) => {
    setOrder(next);
    const mine = ++asked.current;
    void fetchPlan(org, project, pbiId, cases.map((c) => c.id), next.map((c) => c.id)).catch(() => null).then((p) => {
      if (mine !== asked.current) return;
      setFailed(p == null);
      if (p) setPlan(p);
    });
  };

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try {
      await work();
      onClose();
    } catch (e) {
      // The raw error can name the profile folder: it goes to the app log,
      // and the person gets a sentence.
      logUi(`auto-run: the execution order for PBI ${pbiId} was not saved: ${e instanceof Error ? e.message : String(e)}`);
      setError("Could not save the order. Try again, or see Settings → Logs.");
      setBusy(false);
    }
  };
  const save = () => run(() => unwrapStr(commands.autoRunSaveOrder(pbiId, order.map((c) => c.id))));
  const useSuggested = () => run(() => unwrapStr(commands.autoRunClearOrder(pbiId)));

  return (
    <Modal
      onClose={() => !busy && onClose()}
      labelledBy={titleId}
      className={`${MODAL_LARGE} flex flex-col gap-3 p-4`}
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
      {failed && <p className="text-xs text-danger">Could not work out the order.</p>}
      {error && <p className="text-xs text-danger">{error}</p>}
      {loaded && (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <CaseOrderList
            cases={order}
            selected={selected}
            onChange={moved}
            onSelect={setSelected}
            ariaLabel="Auto Run execution order"
            disabled={busy}
            noteBefore={(id) => {
              if (!plan) return null;
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
        {savedOnDisk && (
          <Button variant="outline" size="sm" disabled={busy} onClick={useSuggested}>
            <IconUndo aria-hidden />
            Use suggested order
          </Button>
        )}
        <Button size="sm" disabled={busy || !loaded || failed || unchanged} onClick={save}>
          <IconConfirm aria-hidden />
          Save order
        </Button>
      </div>
    </Modal>
  );
}
