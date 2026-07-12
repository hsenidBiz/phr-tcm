import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type TestCase, type TestCaseFull } from "../bindings";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { useFieldRefs } from "../hooks/useFieldRefs";
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

  const setStep = (i: number, key: "action" | "expected", value: string) =>
    setTc((t) => ({
      ...t,
      steps: t.steps.map((s, j) => (j === i ? { ...s, [key]: value } : s)),
    }));

  const moveStep = (i: number, delta: -1 | 1) =>
    setTc((t) => {
      const steps = [...t.steps];
      const j = i + delta;
      if (j < 0 || j >= steps.length) return t;
      [steps[i], steps[j]] = [steps[j], steps[i]];
      return { ...t, steps };
    });

  return (
    <div className="space-y-2 border-t border-border p-3">
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
          <Input
            aria-label="Module"
            placeholder="Module"
            value={tc.module_value}
            onChange={(e) => setTc((t) => ({ ...t, module_value: e.target.value }))}
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

      <div className="space-y-1">
        {tc.steps.map((s, i) => (
          <div key={i} className="flex items-center gap-1">
            <span className="w-5 text-right text-xs text-faint">{i + 1}</span>
            <Input
              aria-label={`Step ${i + 1} action`}
              className="flex-1 px-2 py-1 text-xs"
              placeholder="Action"
              value={s.action}
              onChange={(e) => setStep(i, "action", e.target.value)}
            />
            <Input
              aria-label={`Step ${i + 1} expected`}
              className="flex-1 px-2 py-1 text-xs"
              placeholder="Expected"
              value={s.expected}
              onChange={(e) => setStep(i, "expected", e.target.value)}
            />
            <button className="px-1 text-xs text-faint hover:text-text" title="Move up" onClick={() => moveStep(i, -1)}>
              ↑
            </button>
            <button className="px-1 text-xs text-faint hover:text-text" title="Move down" onClick={() => moveStep(i, 1)}>
              ↓
            </button>
            <button
              className="px-1 text-xs text-faint hover:text-danger"
              title="Remove step"
              onClick={() => setTc((t) => ({ ...t, steps: t.steps.filter((_, j) => j !== i) }))}
            >
              ✕
            </button>
          </div>
        ))}
        <Button
          variant="ghost"
          size="sm"
          onClick={() =>
            setTc((t) => ({ ...t, steps: [...t.steps, { action: "", expected: "" }] }))
          }
        >
          + Add step
        </Button>
      </div>

      <div className="flex items-center gap-3">
        <Button size="sm" disabled={Boolean(problem) || saveCase.isPending} onClick={() => saveCase.mutate()}>
          {saveCase.isPending ? "Saving..." : "Save changes"}
        </Button>
        {problem && <span className="text-xs text-danger">{problem}</span>}
      </div>
    </div>
  );
}

/** The v1 Edit tab: the PBI's linked cases, editable in place. */
export default function ExistingCases({
  org,
  project,
  pbiId,
}: {
  org: string;
  project: string;
  pbiId: number;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [openId, setOpenId] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const cases = useQuery({
    queryKey: ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(commands.pbiTestCasesFull(org, pbiId, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && pbiId),
    retry: false,
  });

  const exportSel = useMutation({
    mutationFn: async (format: "xlsx" | "json") => {
      const chosen = (cases.data ?? []).filter((c) => selected.has(c.id)).map(toTestCase);
      if (chosen.length === 0) return;
      const path = await save({
        defaultPath: format === "xlsx" ? "test-cases.xlsx" : "test-cases.json",
        filters: [
          format === "xlsx"
            ? { name: "Excel", extensions: ["xlsx"] }
            : { name: "JSON", extensions: ["json"] },
        ],
      });
      if (!path) return;
      const r =
        format === "xlsx"
          ? await commands.exportQueue(path, chosen)
          : await commands.exportQueueJson(path, chosen);
      if (r.status === "error") throw new Error(r.error);
      toast.success(`Exported ${chosen.length} case(s).`);
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  const toggle = (id: number) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-muted">
          Test cases linked to #{pbiId} ({cases.data?.length ?? "..."})
        </h2>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={selected.size === 0}
            onClick={() => exportSel.mutate("xlsx")}
          >
            Export selected xlsx...
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={selected.size === 0}
            onClick={() => exportSel.mutate("json")}
          >
            Export selected JSON...
          </Button>
        </div>
      </div>

      {cases.isLoading && <Skeleton className="h-24" />}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
      {cases.data && cases.data.length === 0 && (
        <p className="text-sm text-muted">No test cases linked yet.</p>
      )}

      <ul className="space-y-1">
        {(cases.data ?? []).map((c) => (
          <li key={c.id} className="rounded-md border border-border">
            <div className="flex items-center gap-2 px-3 py-2 text-sm">
              <input
                type="checkbox"
                aria-label={`Select #${c.id}`}
                checked={selected.has(c.id)}
                onChange={() => toggle(c.id)}
              />
              <button
                className="flex flex-1 items-center gap-2 text-left"
                onClick={() => setOpenId((o) => (o === c.id ? null : c.id))}
              >
                {openId === c.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                <span className="text-faint">#{c.id}</span>
                <span className="text-text">{c.title}</span>
                <span className="ml-auto text-xs text-faint">
                  {c.steps.length} steps · {c.automation_status}
                </span>
              </button>
            </div>
            {openId === c.id && (
              <CaseEditor
                original={c}
                org={org}
                project={project}
                moduleRef={prefs.moduleRef}
                preconditionsRef={prefs.preconditionsRef}
                onSaved={() => qc.invalidateQueries({ queryKey: ["pbi-tcs", org, pbiId] })}
              />
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
