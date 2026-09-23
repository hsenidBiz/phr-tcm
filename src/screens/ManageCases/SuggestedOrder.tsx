import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { commands, type RunOrderCase } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrap } from "../../lib/ipc";
import { loadMyOrder, reconcile, type OrderKey } from "../../lib/runOrder";
import { sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import { noteFor, runOrderQueryOptions } from "../RunPanel/useRunOrder";
import CaseOrderList from "./CaseOrderList";

type StartFrom = "saved" | "azure" | "mine";

/**
 * Suite Management's second order editor for a PBI's suite (design doc
 * §5.3): the suggested run order every tester starts from in Run Tests.
 * SuiteCases renders this only for a requirement suite - a static suite has
 * no PBI and so no suggested order.
 *
 * Reads the exact same query and key as Run Tests (`useRunOrder`'s
 * `runOrderQueryOptions`), so the two screens share one cache entry: a save
 * here shows up there without an extra round trip. Nothing here calls
 * `reorderSuiteCases` - that stays the Azure DevOps order editor's alone.
 */
export default function SuggestedOrder({
  org,
  project,
  planId,
  suiteId,
  pbiId,
  suiteName,
  cases,
}: {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  pbiId: number;
  /** For the case list's accessible name: several lists can be open at once. */
  suiteName: string;
  /** The suite's cases in Azure DevOps' own order - already loaded by
   * SuiteCases, so this component never issues its own suite-cases read. */
  cases: SuiteCase[];
}) {
  const qc = useQueryClient();
  const query = runOrderQueryOptions(org, project, pbiId);
  const runOrder = useQuery(query);
  const read = runOrder.data;
  const file = read?.state === "found" ? read.file : null;
  const unreadable = runOrder.isError || read?.state === "unreadable";
  const reason = runOrder.isError ? runOrder.error.message : read?.state === "unreadable" ? read.reason : null;

  const specIds = cases.map((c) => c.id);
  const byId = new Map(cases.map((c) => [c.id, c]));
  const toCases = (ids: readonly number[]): SuiteCase[] =>
    ids.map((id) => byId.get(id) ?? { id, title: `Test case ${id}` });

  const orderKey: OrderKey = { org, planId, suiteId };
  const myOrder = loadMyOrder(orderKey);

  const idsFor = (from: StartFrom): number[] => {
    if (from === "saved" && file) return reconcile(file.cases.map((c) => c.id), specIds);
    if (from === "mine" && myOrder) return reconcile(myOrder, specIds);
    return specIds;
  };

  const [startFrom, setStartFrom] = useState<StartFrom>(() => (file ? "saved" : "azure"));
  const [order, setOrder] = useState<SuiteCase[]>(() => toCases(idsFor(file ? "saved" : "azure")));
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirming, setConfirming] = useState(false);

  // `touched` is set ONLY by a hand edit or an explicit Start from pick -
  // never by the read settling on its own. While untouched, the list always
  // mirrors the live default (saved file if found, else Azure DevOps order),
  // recomputed whenever the read OR the suite's cases change: a colleague's
  // newer save must reach the screen, and so must a case added or removed
  // from the suite, right up until the tester actually does something.
  const touched = useRef(false);
  useEffect(() => {
    if (runOrder.isLoading || touched.current) return;
    const from: StartFrom = file ? "saved" : "azure";
    setStartFrom(from);
    setOrder(toCases(idsFor(from)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runOrder.isLoading, file, cases]);

  // Once touched, the read no longer drives the list at all - but the
  // suite's cases still can. A case added since lands at the end (in spec
  // order); one removed drops out. The tester's own order among the rest is
  // left exactly as they made it.
  useEffect(() => {
    if (!touched.current) return;
    setOrder((cur) => toCases(reconcile(cur.map((c) => c.id), specIds)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cases]);

  const startOptions: { value: StartFrom; label: string }[] = [
    ...(file ? [{ value: "saved" as const, label: "Saved suggested order" }] : []),
    { value: "azure" as const, label: "Order in Azure DevOps" },
    ...(myOrder ? [{ value: "mine" as const, label: "My order on this machine" }] : []),
  ];

  const changeStartFrom = (from: StartFrom) => {
    touched.current = true; // a deliberate pick beats the read's own default, whenever it lands
    setStartFrom(from);
    setOrder(toCases(idsFor(from)));
  };

  // Any hand edit (drag, arrow) counts as touched too - otherwise a read
  // that resolves right after the first click would silently throw it away.
  const commitOrder = (next: SuiteCase[]) => {
    touched.current = true;
    setOrder(next);
  };

  // Nothing has been saved: whatever is on screen is offerable, so Save is
  // never disabled on that account alone.
  const savedCases = file ? toCases(reconcile(file.cases.map((c) => c.id), specIds)) : null;
  const dirty = savedCases == null || !sameOrder(order, savedCases);

  const save = useMutation({
    mutationFn: () => {
      // Each case keeps the group it had in the saved file; a case with no
      // prior group (or no saved file at all) gets none.
      const groupOf = new Map(file?.cases.map((c) => [c.id, c.group]) ?? []);
      const payload: RunOrderCase[] = order.map((c) => {
        const group = groupOf.get(c.id);
        return group ? { id: c.id, group } : { id: c.id };
      });
      return unwrap(commands.saveRunOrder(org, project, pbiId, payload));
    },
    onSuccess: (newFile) => {
      setConfirming(false);
      toast.success("Suggested run order saved.");
      // So Run Tests (and this editor) see the new file without waiting on
      // a refetch...
      qc.setQueryData(query.queryKey, { state: "found" as const, file: newFile });
      // ...and so the disk copy is rewritten too (the same pairing
      // SuiteCases.tsx uses for Apply order), or Run Tests would paint the
      // old order from disk on its next launch, before it ever asks Azure
      // DevOps again.
      qc.invalidateQueries({ queryKey: query.queryKey });
    },
    onError: (e) => toast.error(`Could not save the suggested run order: ${e.message}`),
  });

  const note = unreadable
    ? noteFor(reason ?? "")
    : file
      ? `Saved by ${file.saved_by} on ${new Date(file.saved_at).toLocaleDateString()}`
      : runOrder.isLoading
        ? null
        : "No suggested run order yet.";

  if (cases.length === 0) return null;

  return (
    <div className="mt-4 space-y-2 border-t border-border pt-4">
      <h3 className="text-sm font-semibold text-text">Suggested run order</h3>
      {note && <p className={cn("text-xs", unreadable ? "text-warning" : "text-muted")}>{note}</p>}
      <div className="flex items-center gap-2">
        <span className="text-xs text-muted">Start from</span>
        <Select
          aria-label="Start from"
          className="w-64"
          triggerClassName="px-2 py-1.5"
          value={startFrom}
          onChange={(e) => changeStartFrom(e.target.value as StartFrom)}
        >
          {startOptions.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
      </div>
      <CaseOrderList
        cases={order}
        selected={selected}
        onChange={commitOrder}
        onSelect={setSelected}
        ariaLabel={`Suggested run order for ${suiteName}`}
        disabled={save.isPending}
      />
      <Button size="sm" disabled={!dirty || save.isPending} onClick={() => setConfirming(true)}>
        <IconConfirm aria-hidden />
        Save suggested order
      </Button>
      {confirming && (
        <Modal onClose={() => setConfirming(false)} className="w-full max-w-md space-y-3 p-5">
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
        </Modal>
      )}
    </div>
  );
}
