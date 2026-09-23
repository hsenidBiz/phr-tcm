// The order Run Tests lists its cases in (design doc §5.1): the PBI's
// suggested run order, the suite's spec order, or this tester's own order
// on this machine. Kept out of the screen so the screen only wires it in.
//
// Shared orders are never written from here: moving a row copies what is
// on screen into My order (local) and switches to it. Nothing in this file
// calls reorderSuiteCases or saveRunOrder.

import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useReducer } from "react";
import { toast } from "sonner";
import { commands, type TestPoint } from "../../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { groupIndices } from "../../lib/grouping";
import { unwrap } from "../../lib/ipc";
import {
  clearMyOrder,
  loadMyOrder,
  loadOrderView,
  onMyOrderChanged,
  reconcile,
  saveMyOrder,
  saveOrderView,
  type OrderKey,
  type OrderView,
} from "../../lib/runOrder";
import { moveBlock, nudgeBlock, type SuiteCase } from "../../lib/suiteOrder";
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
 * it, so a move never swaps with something invisible. */
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

const caseIdsOf = (pts: readonly TestPoint[]): number[] => {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const p of pts) {
    if (p.test_case_id != null && !seen.has(p.test_case_id)) {
      seen.add(p.test_case_id);
      out.push(p.test_case_id);
    }
  }
  return out;
};

/** A reason sentence ends in a full stop before the pointer to the logs;
 * the reasons from the backend are fragments without one. */
const sentence = (s: string) => (/[.!?]$/.test(s.trim()) ? s.trim() : `${s.trim()}.`);

/** The one line under the picker. A reason that already sends the reader
 * to the logs is not told to go there twice. */
function noteFor(reason: string): string {
  const head = `The suggested run order could not be read: ${sentence(reason)}`;
  return /Settings\s*(→|,)\s*Logs/.test(reason) ? head : `${head} See Settings → Logs.`;
}

export function useRunOrder({
  org,
  project,
  pbiId,
  suite,
  points,
  grouped,
}: {
  org: string;
  project: string;
  pbiId: number;
  suite: { plan_id: number; suite_id: number } | undefined;
  points: readonly TestPoint[];
  grouped: boolean;
}) {
  const planId = suite?.plan_id ?? 0;
  const suiteId = suite?.suite_id ?? 0;
  const key: OrderKey | null = suite ? { org, planId, suiteId } : null;
  // Local reads (My order, the chosen view) are re-done on every bump: a
  // move here, a reset, or the runner saving a new My order.
  const [rev, bump] = useReducer((n: number) => n + 1, 0);

  const runOrder = useQuery({
    queryKey: ["run-order", org, project, pbiId],
    ...persistentQuery({
      key: cacheKeys.runOrder(org, project, pbiId),
      fetcher: () => unwrap(commands.getRunOrder(org, project, pbiId)),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    enabled: pbiId > 0,
    retry: false,
  });

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
  const unreadable = runOrder.isError || read?.state === "unreadable";
  const reason = runOrder.isError ? runOrder.error.message : read?.state === "unreadable" ? read.reason : null;
  const note = reason == null ? null : noteFor(reason);

  const keyStr = key ? `${org}/${planId}/${suiteId}` : "";
  const myOrder = useMemo(() => (key ? loadMyOrder(key) : null), [keyStr, rev]);
  const storedView = useMemo(() => (key ? loadOrderView(key) : null), [keyStr, rev]);

  const available = (v: OrderView) => (v === "suggested" ? file != null : v === "mine" ? myOrder != null : true);
  const view: OrderView = storedView && available(storedView) ? storedView : file ? "suggested" : "spec";
  // An unreadable file still lists Suggested, greyed out: the tester sees
  // there is one, and the note says why it is not in use (design doc §6).
  const options: { view: OrderView; disabled: boolean }[] = [
    ...(file || unreadable ? [{ view: "suggested" as const, disabled: !file }] : []),
    { view: "spec" as const, disabled: false },
    ...(myOrder ? [{ view: "mine" as const, disabled: false }] : []),
  ];

  // The runner saves My order when a tester picks "Run next..."; re-read
  // it so this list follows.
  useEffect(() => {
    if (!keyStr) return;
    const un = onMyOrderChanged((k) => {
      if (k.org === org && k.planId === planId && k.suiteId === suiteId) bump();
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [keyStr, org, planId, suiteId]);

  const specIds = useMemo(
    () => specOrderOf(points, suiteCases.data?.map((c) => c.id)),
    [points, suiteCases.data],
  );
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

  const sections = useMemo(
    () => (grouped ? sectionsFor(ordered, groupOf) : [{ name: "", pts: ordered }]),
    [grouped, ordered, groupOf],
  );
  // The order as the eye reads it, before any filter: what a move changes
  // and what gets copied into My order.
  const displayOrder = useMemo(() => caseIdsOf(sections.flatMap((s) => s.pts)), [sections]);

  const commit = (next: SuiteCase[]) => {
    if (!key) return;
    const ids = next.map((c) => c.id);
    saveMyOrder(key, ids);
    // Read it back: with storage unavailable the save is silently dropped,
    // and switching to a My order that is not there would mislead.
    const saved = loadMyOrder(key);
    if (!saved || saved.length !== ids.length || saved.some((id, i) => id !== ids[i])) {
      toast.error("Your own order could not be saved on this machine.");
      return;
    }
    if (view !== "mine") {
      saveOrderView(key, "mine");
      toast.info("Now using your own order, on this machine.");
    }
    bump();
  };
  const asCases = (): SuiteCase[] => displayOrder.map((id) => ({ id, title: "" }));

  /** Where a case sits among its neighbours for Move up / Move down: its
   * section when grouped (a group's rows move within it; the group moves
   * as a block from its header), otherwise the whole list. */
  const siblingsOf = (caseId: number): number[] => {
    const s = sections.find((x) => x.pts.some((p) => p.test_case_id === caseId));
    return s ? caseIdsOf(s.pts) : [];
  };
  const canMove = (caseId: number, dir: "up" | "down") => {
    const sib = siblingsOf(caseId);
    const i = sib.indexOf(caseId);
    return i >= 0 && (dir === "up" ? i > 0 : i < sib.length - 1);
  };
  const moveCase = (caseId: number, dir: "up" | "down") => {
    if (!canMove(caseId, dir)) return;
    commit(nudgeBlock(asCases(), new Set([caseId]), dir));
  };
  const dropCase = (caseId: number, targetId: number) => {
    if (caseId === targetId) return;
    // Grouped, a drop into another section would snap back when the list
    // regroups: the same rule as the arrows, so it is not applied at all.
    if (grouped && !siblingsOf(caseId).includes(targetId)) return;
    commit(moveBlock(asCases(), new Set([caseId]), targetId));
  };

  const sectionIndexOf = (name: string) => sections.findIndex((s) => s.name === name);
  const canMoveGroup = (name: string, dir: "up" | "down") => {
    const i = sectionIndexOf(name);
    return i >= 0 && (dir === "up" ? i > 0 : i < sections.length - 1);
  };
  /** The whole group past its neighbour group, as Suite Management's
   * group arrows do. */
  const moveGroup = (name: string, dir: "up" | "down") => {
    if (!canMoveGroup(name, dir)) return;
    const i = sectionIndexOf(name);
    const members = new Set(caseIdsOf(sections[i].pts));
    const neighbour = caseIdsOf(sections[dir === "up" ? i - 1 : i + 1].pts);
    const target = dir === "up" ? neighbour[0] : neighbour[neighbour.length - 1];
    commit(moveBlock(asCases(), members, target));
  };

  const changeView = (v: OrderView) => {
    if (!key) return;
    saveOrderView(key, v);
    bump();
  };

  const resetLabel = file ? "Reset to suggested order" : "Reset to spec order";
  const reset = () => {
    if (!key) return;
    clearMyOrder(key);
    saveOrderView(key, file ? "suggested" : "spec");
    bump();
  };

  return {
    view,
    options,
    changeView,
    note,
    ordered,
    sections,
    displayOrder,
    canMove,
    moveCase,
    dropCase,
    canMoveGroup,
    moveGroup,
    resetLabel,
    reset,
  };
}
