import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { commands, type SuiteRef } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Select } from "../../components/ui/select";
import { CACHE, persistentQuery } from "../../lib/persistentQuery";
import { buildTree, flattenTree, indented } from "../../lib/suiteTree";
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

/** Two dropdowns over the same cached plan tree the Test Suites tab
 * paints: plan, then suite (indented by depth so it reads as the tree). */
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
  // Seeded from `picked` on mount only; the one caller (ManageCases) never
  // clears `picked` from outside this picker, so it never needs to sync
  // back to a `picked` that changed for some other reason.
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

  // `picked.siblings` is a snapshot from when it was picked. When the plan
  // tree refreshes (e.g. a folder was just created), re-pick the same
  // suite id with the fresh siblings so the rest of the screen sees it.
  useEffect(() => {
    if (!picked || !plans.data) return;
    const p = plans.data.find((x) => x.plan.id === picked.planId);
    const suite = p?.suites.find((s) => s.id === picked.suite.id);
    if (!p || !suite) {
      onPick(null);
      return;
    }
    if (suite !== picked.suite || p.suites !== picked.siblings) {
      onPick({ planId: p.plan.id, planName: p.plan.name, rootSuiteId: p.plan.root_suite_id, suite, siblings: p.suites });
    }
    // onPick is a state setter in the one caller; re-running on its identity would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plans.data, picked?.planId, picked?.suite.id]);

  return (
    <div className="grid gap-3 md:grid-cols-2">
      <label className="block text-xs text-muted">
        Test plan
        <Select
          aria-label="Test plan"
          triggerClassName="mt-1"
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
        </Select>
      </label>
      <label className="block text-xs text-muted">
        Test suite
        <Select
          aria-label="Test suite"
          triggerClassName="mt-1"
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
              {indented(
                suite.suite_type === "requirementTestSuite" && suite.requirement_id != null
                  ? `PBI ${suite.requirement_id}: ${suite.name}`
                  : suite.name,
                depth,
              )}
            </option>
          ))}
        </Select>
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
