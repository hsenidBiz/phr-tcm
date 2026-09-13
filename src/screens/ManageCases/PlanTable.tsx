import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, FolderTree } from "lucide-react";
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { toast } from "sonner";
import { commands, type PlanWithSuites, type SuiteRef } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Select } from "../../components/ui/select";
import { IconCopyToSuite, IconNewFolder } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";
import { buildTree, flattenTree, indented } from "../../lib/suiteTree";
import NewSuiteDialog from "./NewSuiteDialog";
import { selectedIdsIn, type Selection } from "./selection";
import SuiteCases from "./SuiteCases";
import { suiteCasesKey } from "./suiteCasesQuery";

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
  /** Suite ids to open on first render (a picked PBI's own suite). */
  initiallyExpanded: number[];
  selection: Selection | null;
  onToggle: (planId: number, cases: SuiteCase[], on: boolean) => void;
  onClearSelection: () => void;
}) {
  const qc = useQueryClient();
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set(initiallyExpanded));
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
    mutationFn: () =>
      unwrap(commands.addCasesToSuite(org, project, plan.id, Number(target), selectedCases.map((c) => c.id))),
    onSuccess: (added) => {
      const name = staticTargets.find((t) => String(t.id) === target)?.name ?? "the suite";
      toast.success(
        `Copied ${added.length} test case${added.length === 1 ? "" : "s"} to ${name}. ${added.length === 1 ? "It stays" : "They stay"} where ${added.length === 1 ? "it was" : "they were"}.`,
      );
      qc.invalidateQueries({ queryKey: suiteCasesKey(org, project, plan.id, Number(target)) });
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

  return (
    <section aria-label={plan.name} className="rounded-md border border-border bg-surface">
      <header className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <FolderTree size={14} className="text-accent" aria-hidden />
        <span className="font-medium text-text">{plan.name}</span>
        <span className="text-xs text-faint">{plan.area_path}</span>
        <span className="flex-1" />
        {selectedCases.length > 0 && (
          <span className="text-xs text-muted">{selectedCases.length} selected</span>
        )}
        <Select
          aria-label="Copy to"
          triggerClassName="py-1.5"
          value={target}
          disabled={busy || staticTargets.length === 0}
          onChange={(e) => setTarget(e.target.value)}
        >
          <option value="">Pick a suite</option>
          {staticTargets.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || !target || selectedCases.length === 0}
          onClick={() => copy.mutate()}
        >
          <IconCopyToSuite aria-hidden />
          {copy.isPending ? "Copying" : "Copy to suite"}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy || staticTargets.length === 0} onClick={() => setNewOpen(true)}>
          <IconNewFolder aria-hidden />
          New test suite
        </Button>
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
              {open && (
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
              )}
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
            qc.invalidateQueries({ queryKey: ["plans-suites", org, project] });
            onClearSelection();
          }}
        />
      )}
    </section>
  );
}
