import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { commands, type PbiHit } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconClear } from "../../lib/actionIcons";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import PlanTable from "./PlanTable";
import { toggleSelection, type Selection } from "./selection";
import { plansSuitesKey } from "./suiteCasesQuery";

/** A stable empty array: passed as `initiallyExpanded` to every plan that
 * isn't the PBI's own, so `PlanTable`'s effect only re-runs (and its
 * `useMemo`/dependency checks stay cheap) when the set of ids to open
 * actually changes, not on every render of this screen. */
const NONE_EXPANDED: number[] = [];

/** Bulk work on test cases, plan by plan: every suite of a plan with its
 * cases underneath. Cases are selected across a plan's suites and copied
 * into another suite or a new one; each suite's order can be changed and
 * saved. A PBI picked in the bar narrows the view to the plan that holds
 * its suite, with that suite already open. */
export default function ManageCases({ org, project, pbi }: { org: string; project: string; pbi: PbiHit | null }) {
  const plans = useQuery({
    queryKey: plansSuitesKey(org, project),
    ...persistentQuery({
      key: cacheKeys.plansSuites(org, project),
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });

  const [selection, setSelection] = useState<Selection | null>(null);
  const [showAll, setShowAll] = useState(false);
  // A new PBI in the bar narrows the view again.
  useEffect(() => setShowAll(false), [pbi?.id]);

  /** The plan holding the PBI's requirement suite, and that suite. */
  const pbiPlan = useMemo(() => {
    if (!pbi || !plans.data) return null;
    for (const p of plans.data) {
      const suite = p.suites.find((s) => s.suite_type === "requirementTestSuite" && s.requirement_id === pbi.id);
      if (suite) return { plan: p, suiteId: suite.id };
    }
    return null;
  }, [pbi, plans.data]);

  const visible = pbiPlan && !showAll ? [pbiPlan.plan] : (plans.data ?? []);

  /** When the selection's plan has been narrowed out of view (the user
   * picked a PBI whose plan differs from the one holding the selection),
   * name that plan and its case count so the notice can point at it. */
  const hiddenSelection = useMemo(() => {
    if (!selection) return null;
    if (visible.some(({ plan }) => plan.id === selection.planId)) return null;
    const owner = (plans.data ?? []).find(({ plan }) => plan.id === selection.planId);
    if (!owner) return null;
    return { planName: owner.plan.name, count: selection.cases.size };
  }, [selection, visible, plans.data]);

  // A new reference only when the suite id itself changes, so a picked
  // PBI whose suite sits in the SAME plan as before still produces a
  // fresh array `PlanTable` can react to (its effect keys off identity).
  const pbiExpanded = useMemo(() => (pbiPlan ? [pbiPlan.suiteId] : NONE_EXPANDED), [pbiPlan?.suiteId]);

  const onToggle = (planId: number, cases: SuiteCase[], on: boolean) =>
    setSelection((s) => toggleSelection(s, planId, cases, on));

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  return (
    <div className="space-y-3">
      {pbi && plans.data && (
        <div className="flex flex-wrap items-center gap-2 text-sm text-muted">
          <span>
            {pbiPlan
              ? `Showing the plan that holds PBI #${pbi.id}.`
              : `PBI #${pbi.id} has no test suite yet. Showing every plan.`}
          </span>
          {pbiPlan && (
            <Button size="sm" variant="ghost" onClick={() => setShowAll((v) => !v)}>
              {showAll ? "Show only this PBI's plan" : "Show all plans"}
            </Button>
          )}
        </div>
      )}
      {hiddenSelection && (
        <div className="flex flex-wrap items-center gap-2 text-sm text-muted">
          <span>
            {hiddenSelection.count === 1
              ? `1 test case selected in ${hiddenSelection.planName} is out of view.`
              : `${hiddenSelection.count} test cases selected in ${hiddenSelection.planName} are out of view.`}
          </span>
          <Button size="sm" variant="ghost" onClick={() => setSelection(null)}>
            <IconClear aria-hidden />
            Clear selection
          </Button>
        </div>
      )}
      {plans.isFetching && !plans.data && <ScanProgress label="Loading test plans" />}
      {plans.isError && <p className="text-sm text-danger">{plans.error.message}</p>}
      {plans.data && plans.data.length === 0 && (
        <p className="text-sm text-muted">No test plans with test suites in this project yet.</p>
      )}
      {visible.map(({ plan, suites }) => (
        <PlanTable
          key={plan.id}
          org={org}
          project={project}
          plan={plan}
          suites={suites}
          initiallyExpanded={pbiPlan && pbiPlan.plan.plan.id === plan.id ? pbiExpanded : NONE_EXPANDED}
          selection={selection}
          onToggle={onToggle}
          onClearSelection={() => setSelection(null)}
        />
      ))}
    </div>
  );
}
