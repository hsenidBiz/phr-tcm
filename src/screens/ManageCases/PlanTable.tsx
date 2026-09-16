import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, FolderTree } from "lucide-react";
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { toast } from "sonner";
import { commands, type PlanWithSuites, type SuiteRef } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Collapse } from "../../components/ui/collapse";
import Combobox from "../../components/ui/combobox";
import { IconClear, IconCopyToSuite, IconNewSuite } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import { buildTree, flattenTree, indented } from "../../lib/suiteTree";
import NewSuiteDialog from "./NewSuiteDialog";
import { selectedIdsIn, type Selection } from "./selection";
import SuiteCases from "./SuiteCases";
import { plansSuitesKey, suiteCasesKey } from "./suiteCasesQuery";

/** One test plan as a table: its suites in tree order, each a block that
 * opens to show its cases. The header carries the plan's bulk actions,
 * which act on the cases selected in THIS plan: copy them into another
 * suite of the plan, or make a new static suite (with them in it). */
export default function PlanTable({
  org,
  project,
  plan,
  suites,
  initiallyExpanded,
  selection,
  onToggle,
  onClearSelection,
}: {
  org: string;
  project: string;
  plan: PlanWithSuites["plan"];
  suites: SuiteRef[];
  /** Suite ids to open (a picked PBI's own suite): opened at first render
   * and again whenever this array's identity changes, e.g. a newly picked
   * PBI whose suite sits in this same plan. */
  initiallyExpanded: number[];
  selection: Selection | null;
  onToggle: (planId: number, cases: SuiteCase[], on: boolean) => void;
  onClearSelection: () => void;
}) {
  const qc = useQueryClient();
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set(initiallyExpanded));
  // A newly picked PBI can point at a different suite of this SAME plan
  // (the component stays mounted - React keys `PlanTable` by plan id), so
  // opening only has to happen once at construction is not enough. Union
  // the ids in rather than replace: whatever the user opened by hand stays
  // open too.
  useEffect(() => {
    if (initiallyExpanded.length === 0) return;
    setExpanded((s) => {
      const next = new Set(s);
      let changed = false;
      for (const id of initiallyExpanded) {
        if (!next.has(id)) {
          next.add(id);
          changed = true;
        }
      }
      return changed ? next : s;
    });
  }, [initiallyExpanded]);
  const [target, setTarget] = useState("");
  const [newOpen, setNewOpen] = useState(false);

  const rows = useMemo(() => flattenTree(buildTree(suites)), [suites]);
  const selectedIds = selectedIdsIn(selection, plan.id);
  const selectedCases = selection && selection.planId === plan.id ? [...selection.cases.values()] : [];

  /** Static suites, root first: where cases can be copied and suites created. */
  const staticTargets = useMemo(
    () => [
      ...(plan.root_suite_id != null ? [{ id: plan.root_suite_id, label: "Plan root", name: "Plan root" }] : []),
      ...rows
        .filter(({ suite }) => suite.suite_type === "staticTestSuite")
        .map(({ suite, depth }) => ({ id: suite.id, label: indented(suite.name, depth), name: suite.name })),
    ],
    [plan.root_suite_id, rows],
  );
  useEffect(() => {
    if (target && !staticTargets.some((t) => String(t.id) === target)) setTarget("");
  }, [target, staticTargets]);

  const copy = useMutation({
    mutationFn: (suiteId: number) =>
      unwrap(commands.addCasesToSuite(org, project, plan.id, suiteId, selectedCases.map((c) => c.id))),
    onSuccess: (added, suiteId) => {
      const name = staticTargets.find((t) => t.id === suiteId)?.name ?? "the suite";
      toast.success(
        `Copied ${added.length} test case${added.length === 1 ? "" : "s"} to ${name}. ${added.length === 1 ? "It stays" : "They stay"} where ${added.length === 1 ? "it was" : "they were"}.`,
      );
      qc.invalidateQueries({ queryKey: suiteCasesKey(org, project, plan.id, suiteId) });
      onClearSelection();
    },
    onError: (e) => toast.error(`Could not copy the cases: ${e.message}`),
  });

  const toggle = (id: number) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const busy = copy.isPending;

  // Creating a suite needs "Manage test suites" on the plan's area. Asked
  // per plan, because area permissions are per node. Hidden ONLY on a
  // clear no: a check that could not be made must not take away a button
  // the user is entitled to, and the create itself refuses with a message.
  //
  // Memory-only on purpose (react-query's default cache, no persistence):
  // caching a stale "no" across restarts would hide the button after an
  // admin fixed the user's access.
  const mayCreate = useQuery({
    queryKey: ["can-create-suites", org, project, plan.area_path],
    queryFn: () => unwrap(commands.canCreateTestSuites(org, project, plan.area_path)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
    retry: false,
  });
  const hideNewSuite = mayCreate.data === false;

  return (
    <section aria-label={plan.name} className="rounded-md border border-border bg-surface">
      <header className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <FolderTree size={14} className="text-accent" aria-hidden />
        <span className="font-medium text-text">{plan.name}</span>
        <span className="text-xs text-faint">{plan.area_path}</span>
        <span className="flex-1" />
        {selectedCases.length > 0 && (
          <>
            <span className="text-xs text-muted">{selectedCases.length} selected</span>
            <Button size="sm" variant="ghost" onClick={onClearSelection}>
              <IconClear aria-hidden />
              Clear selection
            </Button>
          </>
        )}
        {staticTargets.length > 0 && (
          <Combobox
            ariaLabel="Copy to"
            className="min-w-56"
            triggerClassName="py-1"
            placeholder="Pick a suite"
            value={target}
            onChange={setTarget}
            items={staticTargets.map((t) => ({ value: String(t.id), label: t.label }))}
          />
        )}
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || !target || selectedCases.length === 0}
          onClick={() => copy.mutate(Number(target))}
        >
          <IconCopyToSuite aria-hidden />
          {copy.isPending ? "Copying" : "Copy to suite"}
        </Button>
        {!hideNewSuite && (
          <Button size="sm" variant="ghost" disabled={busy || staticTargets.length === 0} onClick={() => setNewOpen(true)}>
            <IconNewSuite aria-hidden />
            New test suite
          </Button>
        )}
      </header>
      <ul className="p-1">
        {rows.map(({ suite, depth }) => {
          const open = expanded.has(suite.id);
          return (
            <li key={suite.id} className="cv-row" style={{ "--cv-size": "32px" } as CSSProperties}>
              <button
                type="button"
                aria-label={`${open ? "Collapse" : "Expand"} ${suite.name}`}
                aria-expanded={open}
                className={cn(
                  "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft",
                  open && "bg-surface-2",
                )}
                style={{ paddingLeft: 8 + depth * 18 }}
                onClick={() => toggle(suite.id)}
              >
                {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
                {suite.suite_type === "requirementTestSuite" && suite.requirement_id != null && (
                  <Badge className="shrink-0 bg-accent-soft text-accent">PBI {suite.requirement_id}</Badge>
                )}
                <span className="min-w-0 flex-1 break-words">{suite.name}</span>
              </button>
              <Collapse open={open}>
                <div style={{ paddingLeft: 8 + depth * 18 + 20 }}>
                  <SuiteCases
                    org={org}
                    project={project}
                    planId={plan.id}
                    suiteId={suite.id}
                    suiteName={suite.name}
                    selected={selectedIds}
                    onToggle={(cases, on) => onToggle(plan.id, cases, on)}
                  />
                </div>
              </Collapse>
            </li>
          );
        })}
      </ul>
      {newOpen && staticTargets.length > 0 && (
        <NewSuiteDialog
          org={org}
          project={project}
          planId={plan.id}
          parents={staticTargets.map((t) => ({ id: t.id, label: t.id === plan.root_suite_id ? `Plan root (${plan.name})` : t.label }))}
          defaultParentId={staticTargets[0].id}
          caseIds={selectedCases.map((c) => c.id)}
          sourceLabel="their suites"
          onClose={() => setNewOpen(false)}
          onCreated={() => {
            qc.invalidateQueries({ queryKey: plansSuitesKey(org, project) });
            onClearSelection();
          }}
        />
      )}
    </section>
  );
}
