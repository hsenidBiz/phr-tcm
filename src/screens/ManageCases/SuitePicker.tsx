import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { commands, type SuiteRef } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { CACHE, persistentQuery } from "../../lib/persistentQuery";
import { buildTree, flattenTree } from "../../lib/suiteTree";
import { unwrap } from "../../lib/ipc";

/** What the rest of the screen needs to know about the suite in hand. */
export type PickedSuite = {
  planId: number;
  planName: string;
  rootSuiteId: number | null;
  suite: SuiteRef;
  /** Every suite of the plan, flat: folder targets come from here. */
  siblings: SuiteRef[];
};

const INDENT = "    ";

/** Two dropdowns over the same cached plan tree the Test Suites tab
 * paints: plan, then suite (indented by depth so it reads as the tree).
 * Kept as native selects: the list can run to hundreds of suites, and a
 * native control scrolls, types-to-find and reads to a screen reader
 * without any help. */
export default function SuitePicker({
  org,
  project,
  picked,
  onPick,
}: {
  org: string;
  project: string;
  picked: PickedSuite | null;
  onPick: (p: PickedSuite | null) => void;
}) {
  const [planId, setPlanId] = useState<string>(picked ? String(picked.planId) : "");
  const plans = useQuery({
    queryKey: ["plans-suites", org, project],
    ...persistentQuery({
      key: `plans-suites:${org}/${project}`,
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });

  const current = useMemo(
    () => (plans.data ?? []).find((p) => String(p.plan.id) === planId) ?? null,
    [plans.data, planId],
  );
  const rows = useMemo(() => (current ? flattenTree(buildTree(current.suites)) : []), [current]);

  const selectClass =
    "w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text focus:border-accent focus:outline-none disabled:opacity-50";

  return (
    <div className="grid gap-3 md:grid-cols-2">
      <label className="block text-xs text-muted">
        Test plan
        <select
          aria-label="Test plan"
          className={`mt-1 ${selectClass}`}
          value={planId}
          disabled={!plans.data}
          onChange={(e) => {
            setPlanId(e.target.value);
            onPick(null);
          }}
        >
          <option value="">{plans.data ? "Pick a plan" : "Loading plans"}</option>
          {(plans.data ?? []).map(({ plan }) => (
            <option key={plan.id} value={plan.id}>
              {plan.name}
            </option>
          ))}
        </select>
      </label>
      <label className="block text-xs text-muted">
        Test suite
        <select
          aria-label="Test suite"
          className={`mt-1 ${selectClass}`}
          value={picked ? String(picked.suite.id) : ""}
          disabled={!current}
          onChange={(e) => {
            const suite = current?.suites.find((s) => String(s.id) === e.target.value);
            if (!current || !suite) {
              onPick(null);
              return;
            }
            onPick({
              planId: current.plan.id,
              planName: current.plan.name,
              rootSuiteId: current.plan.root_suite_id,
              suite,
              siblings: current.suites,
            });
          }}
        >
          <option value="">Pick a suite</option>
          {rows.map(({ suite, depth }) => (
            <option key={suite.id} value={suite.id}>
              {INDENT.repeat(depth)}
              {suite.suite_type === "requirementTestSuite" && suite.requirement_id != null
                ? `PBI ${suite.requirement_id}: `
                : ""}
              {suite.name}
            </option>
          ))}
        </select>
      </label>
      {plans.isFetching && !plans.data && (
        <div className="md:col-span-2">
          <ScanProgress label="Loading test plans" />
        </div>
      )}
      {plans.isError && <p className="text-sm text-danger md:col-span-2">{plans.error.message}</p>}
    </div>
  );
}
