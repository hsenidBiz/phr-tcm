import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { commands, type PbiHit, type PlanWithSuites } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { IconClear } from "../../lib/actionIcons";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { unwrap } from "../../lib/ipc";
import { projectOfPlan } from "../../lib/plansAll";
import { noProjectSentence } from "../../lib/projectScope";
import type { SuiteCase } from "../../lib/suiteOrder";
import { forgetProjectPlans, usePlansAll } from "../../lib/usePlansAll";
import PlanTable from "./PlanTable";
import { toggleSelection, type Selection } from "./selection";
import { plansSuitesKey } from "./suiteCasesQuery";

/** A stable empty array: passed as `initiallyExpanded` to every plan that
 * isn't the PBI's own, so `PlanTable`'s effect only re-runs (and its
 * `useMemo`/dependency checks stay cheap) when the set of ids to open
 * actually changes, not on every render of this screen. */
const NONE_EXPANDED: number[] = [];

/** A plan with the project that owns it. Every read and write on the plan
 * or its suites goes to that project - never to "the project in the bar". */
type PlanEntry = PlanWithSuites & { project: string };

/** Bulk work on test cases, plan by plan: every suite of a plan with its
 * cases underneath. Cases are selected across a plan's suites and copied
 * into another suite or a new one; each suite's order can be changed and
 * saved. A PBI picked in the bar narrows the view to the plan that holds
 * its suite, with that suite already open. A suite handed over from
 * Search Suites' Manage option wins over that PBI narrowing while it lasts.
 *
 * With the projects shown together and no PBI picked, the plans of every
 * project are listed, each under its project, and each plan is worked on in
 * its own project. A picked PBI keeps today's view: the plans of the PBI's
 * project (`project`), narrowed to the PBI's own plan. */
export default function ManageCases({
  org,
  project,
  allProjects,
  pbi,
  focus,
  onFocusHandled,
}: {
  org: string;
  /** The effective project: the PBI's own while the projects show together,
   * otherwise the picked one. With no PBI, together, it is only the home
   * project and nothing is read for it alone. */
  project: string;
  allProjects: boolean;
  pbi: PbiHit | null;
  /** A suite the user just chose in Search Suites. It wins over the PBI in
   * the bar: it is the thing they clicked. `project` is the project of its
   * plan; a hand-over without it is found by the plan's id. */
  focus?: { planId: number; suiteId: number; project?: string } | null;
  onFocusHandled?: () => void;
}) {
  const qc = useQueryClient();
  // Together, the whole organization is read unless a PBI is picked - and a
  // handed-over suite beats the PBI, so it needs every project's plans to
  // find the one it lives in.
  const together = allProjects && (!pbi || Boolean(focus));
  const own = useQuery({
    queryKey: plansSuitesKey(org, project),
    ...persistentQuery({
      key: cacheKeys.plansSuites(org, project),
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: !together && Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });
  const { projectList, projectNames, plansAll, scanNote } = usePlansAll(org, together);

  /** Every plan in view's source, each with its project. */
  const entries: PlanEntry[] = useMemo(
    () =>
      together
        ? (plansAll.data?.groups ?? []).flatMap((g) => g.plans.map((p) => ({ ...p, project: g.project })))
        : (own.data ?? []).map((p) => ({ ...p, project })),
    [together, plansAll.data, own.data, project],
  );
  const loaded = together ? Boolean(plansAll.data) : Boolean(own.data);
  const fetching = together ? projectList.isFetching || plansAll.isFetching : own.isFetching;
  const listError = together
    ? projectList.isError
      ? projectList.error
      : plansAll.isError
        ? plansAll.error
        : null
    : own.isError
      ? own.error
      : null;
  const coverage = together ? plansAll.data : undefined;

  const [selection, setSelection] = useState<Selection | null>(null);
  const [showAll, setShowAll] = useState(false);
  // A new PBI in the bar, or a newly handed-over suite, narrows the view again.
  useEffect(() => setShowAll(false), [pbi?.id, focus?.planId, focus?.suiteId]);

  /** The plan holding the PBI's requirement suite, and that suite. */
  const pbiPlan = useMemo(() => {
    if (!pbi) return null;
    for (const p of entries) {
      const suite = p.suites.find((s) => s.suite_type === "requirementTestSuite" && s.requirement_id === pbi.id);
      if (suite) return { plan: p, suiteId: suite.id };
    }
    return null;
  }, [pbi, entries]);

  /** The plan the focused suite lives in, if it is still in the tree - the
   * PLAN existing is not enough: the suite itself must still be one of
   * its suites, or there is nothing for the focus to expand. The plan's
   * project is the one the hand-over named, else the one that holds the
   * plan's id. */
  const focusPlan = useMemo(() => {
    if (!focus || !loaded) return null;
    const owner = together ? (focus.project || projectOfPlan(plansAll.data, focus.planId)) : null;
    return (
      entries.find(
        (e) =>
          e.plan.id === focus.planId &&
          (owner === null || e.project === owner) &&
          e.suites.some((s) => s.id === focus.suiteId),
      ) ?? null
    );
  }, [focus, loaded, together, plansAll.data, entries]);

  // A suite handed over from a deleted plan (or one Azure DevOps has since
  // removed) has nothing to show - fall back to the ordinary view rather
  // than an empty screen, and let the caller know it was handled.
  useEffect(() => {
    if (focus && loaded && !focusPlan) onFocusHandled?.();
  }, [focus, loaded, focusPlan, onFocusHandled]);

  const visible: PlanEntry[] =
    focusPlan && !showAll ? [focusPlan] : pbiPlan && !showAll ? [pbiPlan.plan] : entries;

  /** The visible plans by project, in the order they came: a project's
   * heading is shown only when more than one project is in view. */
  const visibleGroups: Array<{ project: string; plans: PlanEntry[] }> = [];
  for (const e of visible) {
    const last = visibleGroups[visibleGroups.length - 1];
    if (last && last.project === e.project) last.plans.push(e);
    else visibleGroups.push({ project: e.project, plans: [e] });
  }
  const showHeaders = new Set(visibleGroups.map((g) => g.project)).size > 1;

  /** When the selection's plan has been narrowed out of view (the user
   * picked a PBI whose plan differs from the one holding the selection),
   * name that plan and its case count so the notice can point at it. */
  const hiddenSelection = useMemo(() => {
    if (!selection) return null;
    if (visible.some(({ plan }) => plan.id === selection.planId)) return null;
    const owner = entries.find(({ plan }) => plan.id === selection.planId);
    if (!owner) return null;
    return { planName: owner.plan.name, count: selection.cases.size };
  }, [selection, visible, entries]);

  // A new reference only when the suite id itself changes, so a picked
  // PBI whose suite sits in the SAME plan as before still produces a
  // fresh array `PlanTable` can react to (its effect keys off identity).
  const pbiExpanded = useMemo(() => (pbiPlan ? [pbiPlan.suiteId] : NONE_EXPANDED), [pbiPlan?.suiteId]);
  // Same idea, for the suite handed over from Search Suites.
  const focusExpanded = useMemo(() => (focus ? [focus.suiteId] : NONE_EXPANDED), [focus?.suiteId]);

  const onToggle = (planId: number, cases: SuiteCase[], on: boolean) =>
    setSelection((s) => toggleSelection(s, planId, cases, on));

  // Together, with no PBI, an organization is all it needs: there is no
  // project to pick. With a PBI whose project is unknown it still needs one.
  if (!org) {
    return (
      <p className="text-sm text-muted">
        {allProjects
          ? "Pick an organization in the bar above to manage test cases."
          : noProjectSentence(false, "to manage test cases")}
      </p>
    );
  }
  if (!together && !project) {
    return (
      <p className="text-sm text-muted">{noProjectSentence(allProjects, "to manage test cases")}</p>
    );
  }

  return (
    <div className="space-y-3">
      {focus && focusPlan && !showAll ? (
        <div className="flex flex-wrap items-center gap-2 text-sm text-muted">
          <span>Showing the suite you picked in Search Suites.</span>
          <Button size="sm" variant="ghost" onClick={() => onFocusHandled?.()}>
            {pbi ? "Show this PBI's plan" : "Show all plans"}
          </Button>
        </div>
      ) : (
        pbi &&
        loaded && (
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
        )
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
      {(together ? !loaded && !listError && (fetching || projectNames.length > 0) : fetching && !loaded) && (
        <ScanProgress label="Loading test plans" />
      )}
      {together && scanNote && <p className="text-xs text-muted">{scanNote}</p>}
      {listError && <p className="text-sm text-danger">{listError.message}</p>}
      {coverage && coverage.shown < coverage.total && (
        <p className="text-xs text-muted">
          Showing plans from {coverage.shown} of {coverage.total} projects.
        </p>
      )}
      {/* One box around the plan tables so the tour can ring what this
          screen IS. The empty-state sentence lives inside it too: a ring
          around an empty box says nothing. */}
      <div data-tour="manage-plans" className="space-y-3">
        {loaded && entries.length === 0 && (
          <p className="text-sm text-muted">
            No test plans with test suites in this {together ? "organization" : "project"} yet.
          </p>
        )}
        {visibleGroups.map((g) => (
          <div key={g.project} className="space-y-3">
            {showHeaders && <h2 className="text-sm font-semibold text-text">{g.project}</h2>}
            {g.plans.map(({ plan, suites }) => (
              <PlanTable
                key={plan.id}
                org={org}
                // The plan's own project, whatever the bar says: its cases,
                // its order, new suites, copies and the create permission
                // are all written there.
                project={g.project}
                plan={plan}
                suites={suites}
                initiallyExpanded={
                  focusPlan && focusPlan.plan.id === plan.id
                    ? focusExpanded
                    : pbiPlan && pbiPlan.plan.plan.id === plan.id
                      ? pbiExpanded
                      : NONE_EXPANDED
                }
                selection={selection}
                onToggle={onToggle}
                onClearSelection={() => setSelection(null)}
                onSuiteCreated={allProjects ? () => forgetProjectPlans(qc, org, g.project) : undefined}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
