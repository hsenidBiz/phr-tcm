// The order Run Tests lists its cases in (run-order design §5.1, as
// amended by the execution-order-modal design): the PBI's suggested run
// order, the suite's spec order, or this tester's own order on this
// machine. Kept out of the screen so the screen only wires it in.
//
// The list never changes an order by itself. The Execution order modal
// picks one: `changeView` for a stored order as it is, `saveMine` for a
// list of the tester's own (My order, on this machine). Nothing here calls
// reorderSuiteCases or saveRunOrder - Save for everyone is the modal's own
// call.

import { useQuery } from "@tanstack/react-query";
import { useMemo, useReducer } from "react";
import { toast } from "../../lib/toast";
import { commands, type TestPoint } from "../../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { groupIndices } from "../../lib/grouping";
import { unwrap } from "../../lib/ipc";
import {
  loadMyOrder,
  loadOrderView,
  reconcile,
  saveMyOrder,
  saveOrderView,
  type GroupMode,
  type OrderKey,
  type OrderView,
} from "../../lib/runOrder";
import type { SuiteCase } from "../../lib/suiteOrder";
import { loadSuiteCases, suiteCasesKey } from "../ManageCases/suiteCasesQuery";

export type PointSection = { name: string; pts: TestPoint[] };

export const ORDER_LABELS: Record<OrderView, string> = {
  suggested: "Suggested run order",
  spec: "Spec order",
  mine: "My order",
};

/** The points' distinct case ids in spec order: by position in the suite's
 * entries when those are known, otherwise (or for a case the entries do
 * not list) in the points' own order. Only cases that have a row are in
 * it, so the order never names something invisible. */
export function specOrderOf(points: readonly TestPoint[], suiteIds: readonly number[] | undefined): number[] {
  const seen = new Set<number>();
  const ids: number[] = [];
  for (const p of points) {
    if (p.test_case_id != null && !seen.has(p.test_case_id)) {
      seen.add(p.test_case_id);
      ids.push(p.test_case_id);
    }
  }
  if (!suiteIds) return ids;
  const at = new Map<number, number>();
  suiteIds.forEach((id, i) => at.set(id, i));
  // Array#sort is stable: cases the entries do not list keep the points'
  // order among themselves, after every listed one.
  return [...ids].sort((a, b) => (at.get(a) ?? Infinity) - (at.get(b) ?? Infinity));
}

/** Points sorted by their case's place in `order`. Stable, so a case's
 * several configurations stay together in their own order; points with no
 * case id go last. */
export function sortPoints(points: readonly TestPoint[], order: readonly number[]): TestPoint[] {
  const at = new Map<number, number>();
  order.forEach((id, i) => at.set(id, i));
  const rank = (p: TestPoint) =>
    p.test_case_id == null ? Number.MAX_SAFE_INTEGER : (at.get(p.test_case_id) ?? Number.MAX_SAFE_INTEGER - 1);
  return [...points].sort((a, b) => rank(a) - rank(b));
}

/** The ordered points as sections: by the suggested file's groups when it
 * has any, else by title. Each section gathers its members and sections
 * follow their first case, so the list reads top to bottom in the order. */
/** `ids` (a permutation of the suite's case ids) in the order Run Tests will
 * actually show them when grouping is on: sectioned the same way `sections`
 * is (`sectionsFor` over the points sorted to `ids`), then flattened back to
 * case ids. Used so a My order saved from a grouped list matches the rows
 * on screen - and so the runner, which ranks by the stored My order, agrees
 * with what the tester saw. */
export function displayOrder(
  ids: readonly number[],
  points: readonly TestPoint[],
  groupOf: Map<number, string> | null,
): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const { pts } of sectionsFor(sortPoints(points, ids), groupOf)) {
    for (const p of pts) {
      if (p.test_case_id != null && !seen.has(p.test_case_id)) {
        seen.add(p.test_case_id);
        out.push(p.test_case_id);
      }
    }
  }
  return out;
}

export function sectionsFor(ordered: readonly TestPoint[], groupOf: Map<number, string> | null): PointSection[] {
  let keys: string[];
  if (groupOf) {
    keys = ordered.map((p) => (p.test_case_id != null ? (groupOf.get(p.test_case_id) ?? "") : ""));
  } else {
    keys = new Array<string>(ordered.length).fill("");
    for (const g of groupIndices(ordered.map((p) => p.test_case_name))) {
      for (const i of g.indices) keys[i] = g.name;
    }
  }
  const byName = new Map<string, TestPoint[]>();
  ordered.forEach((p, i) => {
    const name = keys[i] || "Ungrouped";
    const list = byName.get(name);
    if (list) list.push(p);
    else byName.set(name, [p]);
  });
  return [...byName].map(([name, pts]) => ({ name, pts }));
}

/** A reason sentence ends in a full stop before the pointer to the logs;
 * the reasons from the backend are fragments without one. */
export const sentence = (s: string) => (/[.!?]$/.test(s.trim()) ? s.trim() : `${s.trim()}.`);

/** The one line that says why the suggested run order is not in use: under
 * Run Tests' toolbar and under Start from in the Execution order modal. A
 * reason that already sends the reader to the logs is not told to go there
 * twice. */
function noteFor(reason: string): string {
  const head = `The suggested run order could not be read: ${sentence(reason)}`;
  return /Settings\s*(→|,)\s*Logs/.test(reason) ? head : `${head} See Settings → Logs.`;
}

/** The run-order query exactly as this hook builds it: queryKey and
 * persistentQuery options together, so the Execution order modal's Save
 * for everyone writes into this very cache entry. */
export function runOrderQueryOptions(org: string, project: string, pbiId: number) {
  return {
    queryKey: ["run-order", org, project, pbiId] as const,
    ...persistentQuery({
      key: cacheKeys.runOrder(org, project, pbiId),
      fetcher: () => unwrap(commands.getRunOrder(org, project, pbiId)),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    enabled: pbiId > 0,
    retry: false,
  };
}

export function useRunOrder({
  org,
  project,
  pbiId,
  suite,
  points,
  groupMode,
}: {
  org: string;
  project: string;
  pbiId: number;
  suite: { plan_id: number; suite_id: number } | undefined;
  points: readonly TestPoint[];
  groupMode: GroupMode;
}) {
  const planId = suite?.plan_id ?? 0;
  const suiteId = suite?.suite_id ?? 0;
  const key: OrderKey | null = suite ? { org, planId, suiteId } : null;
  // Local reads (My order, the chosen view) are re-done on every bump: a
  // My order or a view chosen in the Execution order modal.
  const [rev, bump] = useReducer((n: number) => n + 1, 0);

  const runOrder = useQuery(runOrderQueryOptions(org, project, pbiId));

  // The same key and loader as Suite Management, so both screens share one
  // cache entry. While it loads (or if it fails) spec order is the points'
  // own order: the list never waits on it.
  const suiteCases = useQuery({
    queryKey: suiteCasesKey(org, project, planId, suiteId),
    ...persistentQuery({
      key: cacheKeys.suiteCases(org, project, planId, suiteId),
      fetcher: () => loadSuiteCases(org, project, planId, suiteId),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    enabled: Boolean(suite),
    retry: false,
  });

  const read = runOrder.data;
  const file = read?.state === "found" ? read.file : null;
  const reason = runOrder.isError ? runOrder.error.message : read?.state === "unreadable" ? read.reason : null;
  const note = reason == null ? null : noteFor(reason);

  const keyStr = key ? `${org}/${planId}/${suiteId}` : "";
  const myOrder = useMemo(() => (key ? loadMyOrder(key) : null), [keyStr, rev]);
  const storedView = useMemo(() => (key ? loadOrderView(key) : null), [keyStr, rev]);

  const available = (v: OrderView) => (v === "suggested" ? file != null : v === "mine" ? myOrder != null : true);
  const view: OrderView = storedView && available(storedView) ? storedView : file ? "suggested" : "spec";

  const specIds = useMemo(
    () => specOrderOf(points, suiteCases.data?.map((c) => c.id)),
    [points, suiteCases.data],
  );
  // The same cases with their titles, for the Execution order modal's list:
  // a case is titled by its first point's name.
  const specCases = useMemo<SuiteCase[]>(() => {
    const titles = new Map<number, string>();
    for (const p of points) {
      if (p.test_case_id != null && !titles.has(p.test_case_id)) titles.set(p.test_case_id, p.test_case_name);
    }
    return specIds.map((id) => ({ id, title: titles.get(id) ?? `Test case ${id}` }));
  }, [points, specIds]);
  const order = useMemo(() => {
    const chosen =
      view === "suggested" && file ? file.cases.map((c) => c.id) : view === "mine" && myOrder ? myOrder : specIds;
    return reconcile(chosen, specIds);
  }, [view, file, myOrder, specIds]);
  const ordered = useMemo(() => sortPoints(points, order), [points, order]);

  // Areas from the suggested file, when it names one for a case on screen.
  const groupOf = useMemo(() => {
    if (!file) return null;
    const onScreen = new Set(specIds);
    if (!file.cases.some((c) => c.group && onScreen.has(c.id))) return null;
    return new Map(file.cases.map((c) => [c.id, c.group ?? ""]));
  }, [file, specIds]);
  const hasAreas = groupOf != null;

  // none -> one flat section; title -> sectionsFor's own title grouping
  // (groupOf null); area -> the file's areas, falling back to title
  // grouping when this suite's cases on screen carry none.
  const sections = useMemo(() => {
    if (groupMode === "none") return [{ name: "", pts: ordered }];
    return sectionsFor(ordered, groupMode === "area" ? groupOf : null);
  }, [groupMode, ordered, groupOf]);

  /** `ids` as My order on this machine, and My order as the list's order:
   * the Execution order modal's Use this order on a list of the tester's
   * own. False (after saying so) when storage dropped the save - switching
   * to a My order that is not there would mislead. */
  const saveMine = (ids: readonly number[]): boolean => {
    if (!key) return false;
    // Grouping regroups the flat list the modal hands back (`sections`
    // above does the same); store what Run Tests will actually display, or
    // the runner - which ranks upcoming cases by this very order - would
    // follow a sequence the tester never saw. Same grouping the list will
    // show: null for title, the file's areas for area, no regrouping at
    // all (a flat list) for none.
    const toStore =
      groupMode === "none" ? [...ids] : displayOrder(ids, points, groupMode === "area" ? groupOf : null);
    saveMyOrder(key, toStore);
    // Read it back: with storage unavailable the save is silently dropped.
    const saved = loadMyOrder(key);
    if (!saved || saved.length !== toStore.length || saved.some((id, i) => id !== toStore[i])) {
      toast.error("Your own order could not be saved on this machine.");
      return false;
    }
    saveOrderView(key, "mine");
    bump();
    return true;
  };

  /** A stored order as the list's order, as it is. My order stays stored. */
  const changeView = (v: OrderView) => {
    if (!key) return;
    saveOrderView(key, v);
    bump();
  };

  return {
    view,
    changeView,
    saveMine,
    note,
    file,
    loading: runOrder.isLoading,
    myOrder,
    specCases,
    ordered,
    sections,
    hasAreas,
  };
}
