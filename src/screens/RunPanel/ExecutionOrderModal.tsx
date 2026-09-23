import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type RunOrderFile } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconConfirm, IconShare } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { loadWatches } from "../../lib/fileSync";
import { unwrap } from "../../lib/ipc";
import { reconcile, runOrderPayload, type GroupMode, type OrderView } from "../../lib/runOrder";
import { sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import { testerOrderSources } from "../../lib/testerOrderStart";
import CaseOrderList from "../ManageCases/CaseOrderList";
import { ORDER_LABELS, runOrderQueryOptions } from "./useRunOrder";

/** A stored order, or `file:<path>` for one watched draft's tester order. */
type StartFrom = OrderView | `file:${string}`;

const isStored = (from: StartFrom): from is OrderView => from === "suggested" || from === "spec" || from === "mine";

export type ExecutionOrderModalProps = {
  org: string;
  project: string;
  /** 0 when the suite has no PBI: Save for everyone is then absent. */
  pbiId: number;
  /** The cases Run Tests lists, in spec order, with their titles. */
  cases: SuiteCase[];
  /** The list's active order: where Start from begins. */
  view: OrderView;
  file: RunOrderFile | null;
  /** `noteFor(reason)` when the suggested run order could not be read, else null. */
  unreadableNote: string | null;
  /** The run-order read has not settled yet. */
  loading: boolean;
  myOrder: readonly number[] | null;
  /** The grouping mode Run Tests is currently showing; the modal's own
   * choice starts here. */
  groupMode: GroupMode;
  /** Whether the cases on screen carry an area from the suggested run
   * order file - By area is offered only then. */
  hasAreas: boolean;
  /** Use this order on a stored order left exactly as it is: switch the view only. */
  onUseView: (v: OrderView) => void;
  /** Use this order on a list of the tester's own: store it as My order. False when it could not be saved. */
  onUseMine: (ids: number[]) => boolean;
  /** The chosen grouping, applied on Use this order and after a successful
   * Save for everyone; Cancel/close discards it (execution-order-modal design). */
  onGroupMode: (mode: GroupMode) => void;
  onClose: () => void;
};

/**
 * Run Tests' one place to set the execution order (execution-order-modal
 * design §5). Start from a stored order or an uploaded draft's tester
 * order, arrange the cases, then either use the list on this machine or
 * save it as the PBI's suggested run order for every tester.
 *
 * Reordering happens only here: the Run Tests list shows the chosen order
 * and never edits it. Nothing here calls `reorderSuiteCases` - the order in
 * Azure DevOps stays Suite Management's alone.
 */
export default function ExecutionOrderModal({
  org,
  project,
  pbiId,
  cases,
  view,
  file,
  unreadableNote,
  loading,
  myOrder,
  groupMode,
  hasAreas,
  onUseView,
  onUseMine,
  onGroupMode,
  onClose,
}: ExecutionOrderModalProps) {
  const qc = useQueryClient();
  const specIds = useMemo(() => cases.map((c) => c.id), [cases]);
  const byId = useMemo(() => new Map(cases.map((c) => [c.id, c])), [cases]);
  const toCases = (ids: readonly number[]): SuiteCase[] =>
    ids.map((id) => byId.get(id) ?? { id, title: `Test case ${id}` });

  // An optimized draft whose cases were all uploaded earlier: the upload
  // saved no suggested order for it, but the Import tab still remembers the
  // file with its ids and tester order, so it can be started from here.
  const testerSources = useMemo(
    () => testerOrderSources(loadWatches(org, pbiId), specIds),
    [org, pbiId, specIds],
  );
  const testerSourceFor = (from: StartFrom) => testerSources.find((s) => `file:${s.path}` === from);

  /** The list a start gives, reconciled against the cases (design doc §4.4). */
  const idsFor = (from: StartFrom): number[] => {
    if (from === "suggested" && file) return reconcile(file.cases.map((c) => c.id), specIds);
    if (from === "mine" && myOrder) return reconcile(myOrder, specIds);
    const tester = testerSourceFor(from);
    return tester ? reconcile(tester.ids, specIds) : specIds;
  };

  const [startFrom, setStartFrom] = useState<StartFrom>(view);
  const [order, setOrder] = useState<SuiteCase[]>(() => toCases(idsFor(view)));
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirming, setConfirming] = useState(false);
  // The current mode, unless it is Area with nothing to group by on screen
  // - then the picker starts at By title rather than a choice it cannot offer.
  const [chosenGroupMode, setChosenGroupMode] = useState<GroupMode>(() =>
    groupMode === "area" && !hasAreas ? "title" : groupMode,
  );

  const groupOptions: { value: GroupMode; label: string }[] = [
    { value: "none", label: "Don't group" },
    { value: "title", label: "By title" },
    ...(hasAreas ? [{ value: "area" as const, label: "By area" }] : []),
  ];

  // An unreadable file still lists Suggested, greyed out: the tester sees
  // there is one, and the note says why it cannot be used.
  const startOptions: { value: StartFrom; label: string; disabled: boolean }[] = [
    ...(file || unreadableNote != null
      ? [{ value: "suggested" as const, label: ORDER_LABELS.suggested, disabled: !file }]
      : []),
    { value: "spec", label: ORDER_LABELS.spec, disabled: false },
    ...(myOrder ? [{ value: "mine" as const, label: ORDER_LABELS.mine, disabled: false }] : []),
    ...testerSources.map((s) => ({ value: `file:${s.path}` as const, label: s.label, disabled: false })),
  ];

  const changeStartFrom = (from: StartFrom) => {
    setStartFrom(from);
    setOrder(toCases(idsFor(from)));
    setSelected(new Set());
  };

  // Save for everyone is pointless when it would only re-stamp saved_by and
  // saved_at on the file already there - the old Suite Management editor
  // disabled its own Save the same way. Spec order or My order happening to
  // match by coincidence still counts as a genuine save (a different start
  // becoming the suggested order is a real change); only Suggested left
  // untouched is a no-op.
  const unchangedFromSaved = startFrom === "suggested" && file != null && sameOrder(order, toCases(idsFor("suggested")));

  // A stored order left exactly as it is changes only which order the list
  // shows, so a stored My order survives a switch to Suggested or Spec.
  // Anything else is the tester's own list.
  const applyOrder = () => {
    onGroupMode(chosenGroupMode);
    if (isStored(startFrom) && sameOrder(order, toCases(idsFor(startFrom)))) {
      onUseView(startFrom);
      onClose();
    } else if (onUseMine(order.map((c) => c.id))) {
      onClose();
    }
  };

  const runOrderKey = runOrderQueryOptions(org, project, pbiId).queryKey;
  const save = useMutation({
    mutationFn: () =>
      unwrap(
        commands.saveRunOrder(
          org,
          project,
          pbiId,
          runOrderPayload(
            order.map((c) => c.id),
            file?.cases ?? null,
            testerSourceFor(startFrom)?.groups,
          ),
        ),
      ),
    onSuccess: (newFile) => {
      toast.success("Suggested run order saved.");
      // Run Tests sees the new file at once...
      qc.setQueryData(runOrderKey, { state: "found" as const, file: newFile });
      // ...and the disk copy is rewritten too, or the next launch would
      // paint the old order from disk before asking Azure DevOps again.
      qc.invalidateQueries({ queryKey: runOrderKey });
      onGroupMode(chosenGroupMode);
      onUseView("suggested");
      onClose();
    },
    onError: (e) => {
      setConfirming(false);
      toast.error(`Could not save the suggested run order: ${e.message}`);
    },
  });

  // Not while a save is on its way: its answer has to land on this modal.
  const close = () => {
    if (!save.isPending) onClose();
  };

  const note =
    unreadableNote ??
    (file
      ? `Saved by ${file.saved_by} on ${new Date(file.saved_at).toLocaleDateString()}`
      : loading
        ? null
        : "No suggested run order yet.");

  return (
    <Modal onClose={close} className="flex max-h-[85vh] w-[640px] max-w-full flex-col gap-3 p-4">
      <h2 className="text-sm font-semibold text-text">Execution order</h2>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-muted">Start from</span>
        <Select
          aria-label="Start from"
          className="w-72"
          triggerClassName="px-2 py-1.5"
          value={startFrom}
          disabled={save.isPending}
          onChange={(e) => changeStartFrom(e.target.value as StartFrom)}
        >
          {startOptions.map((o) => (
            <option key={o.value} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </Select>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-muted">Group cases</span>
        <Select
          aria-label="Group cases"
          className="w-56"
          triggerClassName="px-2 py-1.5"
          value={chosenGroupMode}
          disabled={save.isPending}
          onChange={(e) => setChosenGroupMode(e.target.value as GroupMode)}
        >
          {groupOptions.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
      </div>
      {note && <p className={cn("text-xs", unreadableNote ? "text-warning" : "text-muted")}>{note}</p>}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <CaseOrderList
          cases={order}
          selected={selected}
          onChange={setOrder}
          onSelect={setSelected}
          ariaLabel="Execution order"
          disabled={save.isPending}
        />
      </div>
      {confirming ? (
        <div className="space-y-3 rounded-md border border-border bg-surface-2 p-3">
          <p className="text-sm text-text">Every tester will see this as the suggested run order for this PBI.</p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" disabled={save.isPending} onClick={() => setConfirming(false)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
              <IconConfirm aria-hidden />
              {save.isPending ? "Saving" : "Save"}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={close}>
            <IconCancel aria-hidden />
            Cancel
          </Button>
          {pbiId > 0 && (
            <Button
              variant="outline"
              size="sm"
              disabled={unchangedFromSaved}
              title={
                unchangedFromSaved
                  ? "This is already the suggested run order - move a case first"
                  : "Save as the suggested run order every tester starts from"
              }
              onClick={() => setConfirming(true)}
            >
              <IconShare aria-hidden />
              Save for everyone
            </Button>
          )}
          <Button size="sm" onClick={applyOrder}>
            <IconConfirm aria-hidden />
            Use this order
          </Button>
        </div>
      )}
    </Modal>
  );
}
