import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, Folder, FolderOpen, FolderTree, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { commands, events, type SuiteRef, type TestCase } from "../bindings";
import { Badge } from "../components/ui/badge";
import { Input } from "../components/ui/input";
import { cn } from "../lib/cn";
import { unwrap, unwrapStr } from "../lib/ipc";
import { outcomeLabel } from "./RunPanel";

const outcomeColor: Record<string, string> = {
  passed: "text-success",
  failed: "text-danger",
  blocked: "text-warning",
  notapplicable: "text-faint",
};

type SuiteNode = { suite: SuiteRef; children: SuiteNode[] };

/** v1 folder-structure parity: rebuild the multi-level tree from parent
 * links (the root suite is already stripped in Rust, so parent_id null =
 * top level). */
function buildTree(suites: SuiteRef[]): SuiteNode[] {
  const nodes = new Map<number, SuiteNode>();
  for (const s of suites) nodes.set(s.id, { suite: s, children: [] });
  const roots: SuiteNode[] = [];
  for (const n of nodes.values()) {
    const pid = n.suite.parent_id;
    if (pid != null && nodes.has(pid)) nodes.get(pid)!.children.push(n);
    else roots.push(n);
  }
  return roots;
}

function descendantIds(n: SuiteNode): number[] {
  return [n.suite.id, ...n.children.flatMap(descendantIds)];
}

/** Keep nodes whose name matches `q` (with their whole subtree) or that
 * contain a matching descendant (pruned to the matching branches). */
function pruneTree(nodes: SuiteNode[], q: string): SuiteNode[] {
  return nodes
    .map((n) => {
      if (n.suite.name.toLowerCase().includes(q)) return n;
      const kids = pruneTree(n.children, q);
      return kids.length ? { suite: n.suite, children: kids } : null;
    })
    .filter((n): n is SuiteNode => n !== null);
}

function SuitePoints({
  org,
  project,
  planId,
  suite,
}: {
  org: string;
  project: string;
  planId: number;
  suite: SuiteRef;
}) {
  const points = useQuery({
    queryKey: ["points", org, project, planId, suite.id],
    queryFn: () => unwrap(commands.listTestPoints(org, project, planId, suite.id)),
    retry: false,
  });

  if (points.isLoading) return <p className="ml-8 text-sm text-muted">Loading test points</p>;
  if (points.isError)
    return <p className="ml-8 text-sm text-danger">{points.error.message}</p>;
  if (!points.data || points.data.length === 0)
    return <p className="ml-8 text-sm text-muted">No test points in this suite.</p>;

  return (
    <table className="ml-8 w-[calc(100%-2rem)] border-collapse text-sm">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted">
          <th className="px-2 py-1 font-medium">Test case</th>
          <th className="px-2 py-1 font-medium">Configuration</th>
          <th className="px-2 py-1 font-medium">Last outcome</th>
        </tr>
      </thead>
      <tbody>
        {points.data.map((p) => (
          <tr key={p.point_id} className="border-b border-border/50 hover:bg-surface-2">
            <td className="px-2 py-1 text-text">
              <span className="id-mono text-faint">#{p.test_case_id}</span> {p.test_case_name}
            </td>
            <td className="px-2 py-1 text-muted">{p.config_name}</td>
            <td className={cn("px-2 py-1", outcomeColor[p.last_outcome.toLowerCase()] ?? "text-faint")}>
              {outcomeLabel(p.last_outcome) || "—"}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

type SuiteAction = { planId: number; suiteIds: number[]; label: string };

/** The v1 Test Suites browser: plan -> multi-level suite tree (plans with
 * no suites are hidden - the v1 rule). Folders collapse; a suite click
 * shows its points; folders and suites can be viewed in the browser or
 * handed to Edit Test Cases; requirement suites also jump to Run. */
export default function Suites({
  org,
  project,
  onOpenPbi,
  onEditCases,
}: {
  org: string;
  project: string;
  onOpenPbi?: (pbi: { id: number; title: string }, target: "edit" | "run") => void;
  onEditCases?: (label: string, caseIds: number[]) => void;
}) {
  const qc = useQueryClient();
  const [openSuite, setOpenSuite] = useState<number | null>(null);
  // Folders start collapsed; clicking a folder row toggles it open.
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [search, setSearch] = useState("");
  const [scan, setScan] = useState<{ done: number; total: number } | null>(null);

  // Scanning every plan is the expensive part - cache the result for the
  // session and re-scan only on explicit Refresh.
  const plans = useQuery({
    queryKey: ["plans-suites", org, project],
    queryFn: () => unwrap(commands.listPlansWithSuites(org, project)),
    enabled: Boolean(org && project),
    staleTime: Infinity,
    retry: false,
  });

  useEffect(() => {
    const un = events.suiteScanProgress.listen((e) => setScan(e.payload));
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const trees = useMemo(
    () => (plans.data ?? []).map(({ plan, suites }) => ({ plan, roots: buildTree(suites) })),
    [plans.data],
  );

  const q = search.trim().toLowerCase();
  const visibleTrees = useMemo(() => {
    if (!q) return trees;
    return trees
      .map(({ plan, roots }) => {
        // A matching plan name keeps the whole plan; otherwise prune to
        // the suites (or branches) that match.
        if (plan.name.toLowerCase().includes(q)) return { plan, roots };
        const pruned = pruneTree(roots, q);
        return pruned.length ? { plan, roots: pruned } : null;
      })
      .filter((t): t is (typeof trees)[number] => t !== null);
  }, [trees, q]);

  /** Collect the distinct test case ids under the given suites (a folder
   * action passes all its descendants). */
  const gatherCaseIds = async (planId: number, suiteIds: number[]) => {
    const ids = new Set<number>();
    for (const sid of suiteIds) {
      const pts = await unwrap(commands.listTestPoints(org, project, planId, sid));
      for (const p of pts) if (p.test_case_id != null) ids.add(p.test_case_id);
    }
    return [...ids];
  };

  const view = useMutation({
    mutationFn: async ({ planId, suiteIds, label }: SuiteAction) => {
      const ids = await gatherCaseIds(planId, suiteIds);
      if (ids.length === 0) throw new Error("No test cases under this item.");
      const cases = await unwrap(commands.testCasesByIds(org, ids, null, null));
      const queue: TestCase[] = cases.map((c) => ({
        title: c.title,
        steps: c.steps,
        tags: c.tags,
        automation_status: c.automation_status,
        module_value: c.module_value,
        preconditions: c.preconditions,
        update_id: c.id,
      }));
      await unwrapStr(commands.viewQueueHtml(queue, label));
    },
    onError: (e) => toast.error(e.message),
  });

  const edit = useMutation({
    mutationFn: async (a: SuiteAction) => ({ label: a.label, ids: await gatherCaseIds(a.planId, a.suiteIds) }),
    onSuccess: ({ label, ids }) => {
      if (ids.length === 0) {
        toast.info("No test cases under this item.");
        return;
      }
      onEditCases?.(label, ids);
    },
    onError: (e) => toast.error(e.message),
  });

  const busy = view.isPending || edit.isPending;

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to browse test suites.
      </p>
    );
  }

  const chip = (text: string, onClick: () => void) => (
    <span
      role="button"
      className={cn(
        "rounded border border-border px-1.5 py-0.5 text-[10px] text-muted hover:border-accent hover:text-accent",
        busy && "pointer-events-none opacity-50",
      )}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {text}
    </span>
  );

  const renderNode = (node: SuiteNode, planId: number, depth: number): ReactNode => {
    const s = node.suite;
    const isFolder = node.children.length > 0;
    // While searching, matches are always shown expanded.
    const isCollapsed = q ? false : !expanded.has(s.id);
    const allIds = descendantIds(node);

    return (
      <li key={s.id}>
        <button
          className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
          style={{ paddingLeft: 8 + depth * 18 }}
          onClick={() =>
            isFolder
              ? setExpanded((c) => {
                  const next = new Set(c);
                  if (next.has(s.id)) next.delete(s.id);
                  else next.add(s.id);
                  return next;
                })
              : setOpenSuite((o) => (o === s.id ? null : s.id))
          }
        >
          {isFolder ? (
            <>
              {isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
              {isCollapsed ? (
                <Folder size={14} className="text-warning" />
              ) : (
                <FolderOpen size={14} className="text-warning" />
              )}
            </>
          ) : openSuite === s.id ? (
            <ChevronDown size={14} />
          ) : (
            <ChevronRight size={14} />
          )}
          {s.name}
          {s.suite_type === "requirementTestSuite" && (
            <Badge className="bg-accent-soft text-accent">PBI {s.requirement_id}</Badge>
          )}
          <span className="ml-auto flex gap-1">
            {chip("View", () => view.mutate({ planId, suiteIds: allIds, label: s.name }))}
            {onOpenPbi && s.suite_type === "requirementTestSuite" && s.requirement_id ? (
              <>
                {chip("Edit cases", () =>
                  onOpenPbi({ id: s.requirement_id!, title: s.name }, "edit"),
                )}
                {chip("Run", () => onOpenPbi({ id: s.requirement_id!, title: s.name }, "run"))}
              </>
            ) : (
              onEditCases &&
              chip("Edit cases", () => edit.mutate({ planId, suiteIds: allIds, label: s.name }))
            )}
          </span>
        </button>
        {!isFolder && openSuite === s.id && (
          <SuitePoints org={org} project={project} planId={planId} suite={s} />
        )}
        {isFolder && !isCollapsed && (
          <ul>{node.children.map((c) => renderNode(c, planId, depth + 1))}</ul>
        )}
      </li>
    );
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">Test plans</h2>
        <button
          aria-label="Refresh test plans"
          title="Refresh test plans"
          className="rounded p-1 text-muted hover:text-accent"
          onClick={() => {
            setScan(null);
            qc.invalidateQueries({ queryKey: ["plans-suites", org, project] });
            qc.invalidateQueries({ queryKey: ["points"] });
          }}
        >
          <RefreshCw size={14} />
        </button>
        <Input
          aria-label="Search suites"
          className="w-64 px-2 py-1"
          placeholder="Search plans and suites"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        {busy && <span className="text-xs text-muted">Collecting test cases</span>}
      </div>

      {plans.isFetching && (
        <p className="text-sm text-muted">
          {scan ? `Scanning test plans ${scan.done} of ${scan.total}` : "Loading test plans"}
        </p>
      )}
      {plans.isError && <p className="text-sm text-danger">{plans.error.message}</p>}
      {plans.data && plans.data.length === 0 && (
        <p className="text-sm text-muted">No test plans with test suites in this project yet.</p>
      )}
      {q && (plans.data?.length ?? 0) > 0 && visibleTrees.length === 0 && (
        <p className="text-sm text-muted">Nothing matches "{search.trim()}".</p>
      )}

      {visibleTrees.map(({ plan, roots }) => (
        <section key={plan.id} className="rounded-md border border-border bg-surface">
          <header className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm font-medium text-text">
            <FolderTree size={14} className="text-accent" />
            {plan.name}
            <span className="text-xs text-faint">{plan.area_path}</span>
          </header>
          <ul className="p-1">{roots.map((n) => renderNode(n, plan.id, 0))}</ul>
        </section>
      ))}
    </div>
  );
}
