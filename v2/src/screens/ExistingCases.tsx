import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type TestCase, type TestCaseFull } from "../bindings";
import BulkEditDialog from "../components/BulkEditDialog";
import ModuleField from "../components/ModuleField";
import StepsEditor from "../components/StepsEditor";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { cn } from "../lib/cn";
import { groupIndices } from "../lib/grouping";
import { unwrap } from "../lib/ipc";
import { validateCase } from "../lib/validate";

function toTestCase(c: TestCaseFull): TestCase {
  return {
    title: c.title,
    steps: c.steps,
    tags: c.tags,
    automation_status: c.automation_status,
    module_value: c.module_value,
    preconditions: c.preconditions,
    update_id: c.id,
  };
}

function CaseEditor({
  original,
  org,
  project,
  moduleRef,
  preconditionsRef,
  onSaved,
}: {
  original: TestCaseFull;
  org: string;
  project: string;
  moduleRef: string | null;
  preconditionsRef: string | null;
  onSaved: () => void;
}) {
  const [tc, setTc] = useState<TestCase>(() => toTestCase(original));
  const problem = validateCase(tc);

  const saveCase = useMutation({
    mutationFn: async () => {
      const r = await commands.updateTestCase(org, project, tc, moduleRef, preconditionsRef);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: () => {
      toast.success(`Updated #${original.id}: ${tc.title}`);
      onSaved();
    },
    onError: (e) => toast.error(`Save failed: ${e.message}`),
  });

  return (
    <div className="space-y-2 border-t border-border p-3" onClick={(e) => e.stopPropagation()}>
      <div className="flex gap-2">
        <Input
          aria-label="Case title"
          className="flex-1"
          value={tc.title}
          onChange={(e) => setTc((t) => ({ ...t, title: e.target.value }))}
        />
        <Select
          aria-label="Automation status"
          value={tc.automation_status}
          onChange={(e) => setTc((t) => ({ ...t, automation_status: e.target.value }))}
        >
          <option>Not Automated</option>
          <option>Planned</option>
        </Select>
      </div>
      <div className="flex gap-2">
        <Input
          aria-label="Tags"
          className="flex-1"
          placeholder="Tags (semicolon-separated)"
          value={tc.tags}
          onChange={(e) => setTc((t) => ({ ...t, tags: e.target.value }))}
        />
        {moduleRef && (
          <ModuleField
            org={org}
            project={project}
            value={tc.module_value}
            onChange={(v) => setTc((t) => ({ ...t, module_value: v }))}
          />
        )}
      </div>
      {preconditionsRef && (
        <Textarea
          aria-label="Preconditions"
          className="h-16 w-full"
          placeholder="Preconditions"
          value={tc.preconditions}
          onChange={(e) => setTc((t) => ({ ...t, preconditions: e.target.value }))}
        />
      )}

      <StepsEditor steps={tc.steps} onChange={(steps) => setTc((t) => ({ ...t, steps }))} />

      <div className="flex items-center gap-3">
        <Button size="sm" disabled={Boolean(problem) || saveCase.isPending} onClick={() => saveCase.mutate()}>
          {saveCase.isPending ? "Saving" : "Save changes"}
        </Button>
        {problem && <span className="text-xs text-danger">{problem}</span>}
      </div>
    </div>
  );
}

/** The Edit tab: click selects a card, ctrl+click toggles, shift+click
 * ranges; the chevron (or double-click) expands the editor. Selection
 * unlocks the bulk toolbar. Grouping by module is the v1 smart grouping. */
export default function ExistingCases({
  org,
  project,
  pbiId,
  caseIds,
  label,
}: {
  org: string;
  project: string;
  pbiId: number | null;
  /** When set, edit these exact cases (suite handoff) instead of a PBI's. */
  caseIds?: number[];
  label?: string;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [openId, setOpenId] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-cases") === "on",
  );

  const queryKey = caseIds
    ? ["cases-by-ids", org, caseIds, prefs.moduleRef, prefs.preconditionsRef]
    : ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef];

  const cases = useQuery({
    queryKey,
    queryFn: () =>
      caseIds
        ? unwrap(commands.testCasesByIds(org, caseIds, prefs.moduleRef, prefs.preconditionsRef))
        : unwrap(commands.pbiTestCasesFull(org, pbiId!, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && (caseIds ? caseIds.length > 0 : pbiId != null)),
    retry: false,
  });

  const list = cases.data ?? [];
  const ordered = useMemo(() => {
    if (!grouped) return [{ group: "", items: list }];
    // v1 smart grouping: cluster by shared title prefixes.
    return groupIndices(list.map((c) => c.title)).map(({ name, indices }) => ({
      group: name || "Ungrouped",
      items: indices.map((i) => list[i]),
    }));
  }, [list, grouped]);
  const flat = useMemo(() => ordered.flatMap((g) => g.items), [ordered]);

  const handleCardClick = (c: TestCaseFull, e: React.MouseEvent) => {
    const idx = flat.findIndex((x) => x.id === c.id);
    if (e.shiftKey && anchor != null) {
      const a = flat.findIndex((x) => x.id === anchor);
      if (a >= 0 && idx >= 0) {
        const [lo, hi] = a < idx ? [a, idx] : [idx, a];
        const range = flat.slice(lo, hi + 1).map((x) => x.id);
        setSelected((s) => (e.ctrlKey || e.metaKey ? new Set([...s, ...range]) : new Set(range)));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      setSelected((s) => {
        const next = new Set(s);
        if (next.has(c.id)) next.delete(c.id);
        else next.add(c.id);
        return next;
      });
    } else {
      setSelected(new Set([c.id]));
    }
    setAnchor(c.id);
  };

  const selectedCases = list.filter((c) => selected.has(c.id));

  const exportJson = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-cases.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const r = await commands.exportQueueJson(path, selectedCases.map(toTestCase));
      if (r.status === "error") throw new Error(r.error);
      toast.success(`Exported ${selectedCases.length} case(s).`);
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  const viewHtml = useMutation({
    mutationFn: async () => {
      const chosen = selectedCases.length > 0 ? selectedCases : list;
      const r = await commands.viewQueueHtml(
        chosen.map(toTestCase),
        label ?? (pbiId != null ? `PBI #${pbiId}` : ""),
      );
      if (r.status === "error") throw new Error(r.error);
    },
    onError: (e) => toast.error(`Could not open the report: ${e.message}`),
  });

  const refresh = () => {
    qc.invalidateQueries({ queryKey });
    qc.invalidateQueries({ queryKey: ["pbi-tc-titles", org, pbiId] });
  };

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">
          {label ?? `Test cases linked to #${pbiId}`} ({list.length})
        </h2>
        <button
          aria-label="Refresh"
          title="Refresh"
          className="rounded p-1 text-muted hover:text-accent"
          onClick={refresh}
        >
          <RefreshCw size={14} />
        </button>
        <label className="flex items-center gap-1.5 text-xs text-muted">
          <input
            type="checkbox"
            checked={grouped}
            onChange={(e) => {
              setGrouped(e.target.checked);
              try {
                localStorage.setItem("tcm-v2-group-cases", e.target.checked ? "on" : "off");
              } catch {
                // session-only
              }
            }}
          />
          Group by title
        </label>
        <div className="ml-auto">
          <Button variant="outline" size="sm" disabled={list.length === 0} onClick={() => viewHtml.mutate()}>
            View in browser
          </Button>
        </div>
      </div>

      {selected.size > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-accent/40 bg-accent-soft px-3 py-1.5 text-sm">
          <span className="font-medium text-accent">{selected.size} selected</span>
          <Button size="sm" onClick={() => setBulkOpen(true)}>
            Bulk edit
          </Button>
          <Button variant="outline" size="sm" onClick={() => exportJson.mutate()}>
            Export JSON
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
          <span className="ml-auto text-xs text-faint">Ctrl+click to toggle · Shift+click for range</span>
        </div>
      )}

      {cases.isLoading && <Skeleton className="h-24" />}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
      {cases.data && list.length === 0 && (
        <p className="text-sm text-muted">No test cases here yet.</p>
      )}

      {ordered.map(({ group, items }) => (
        <div key={group || "__all"} className="space-y-1">
          {group && (
            <div className="pt-1 text-xs font-semibold uppercase tracking-wide text-faint">
              {group} <span className="normal-case">({items.length})</span>
            </div>
          )}
          <ul className="space-y-1">
            {items.map((c) => (
              <li
                key={c.id}
                className={cn(
                  "cursor-pointer select-none rounded-md border transition-colors",
                  selected.has(c.id)
                    ? "border-accent bg-accent-soft"
                    : "border-border hover:border-border-strong",
                )}
                onClick={(e) => handleCardClick(c, e)}
                onDoubleClick={() => setOpenId((o) => (o === c.id ? null : c.id))}
              >
                <div className="flex items-center gap-2 px-3 py-2 text-sm">
                  <button
                    aria-label={`Expand #${c.id}`}
                    className="text-muted hover:text-accent"
                    onClick={(e) => {
                      e.stopPropagation();
                      setOpenId((o) => (o === c.id ? null : c.id));
                    }}
                  >
                    {openId === c.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  </button>
                  <span className="id-mono text-faint">#{c.id}</span>
                  <span className="text-text">{c.title}</span>
                  <span className="ml-auto text-xs text-faint">
                    {c.steps.length} steps · {c.automation_status}
                  </span>
                </div>
                {openId === c.id && (
                  <CaseEditor
                    original={c}
                    org={org}
                    project={project}
                    moduleRef={prefs.moduleRef}
                    preconditionsRef={prefs.preconditionsRef}
                    onSaved={refresh}
                  />
                )}
              </li>
            ))}
          </ul>
        </div>
      ))}

      {bulkOpen && (
        <BulkEditDialog
          org={org}
          project={project}
          cases={selectedCases}
          onClose={() => setBulkOpen(false)}
          onDone={() => {
            setBulkOpen(false);
            setSelected(new Set());
            refresh();
          }}
        />
      )}
    </section>
  );
}
