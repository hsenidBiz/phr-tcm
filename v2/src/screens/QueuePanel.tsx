import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands, events, type SubmitItemResult, type TestCase } from "../bindings";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { duplicateWarning, validateCase } from "../lib/validate";

function parseStepsText(text: string) {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .map((line) => {
      const [action, expected = ""] = line.split("=>");
      return { action: action.trim(), expected: expected.trim() };
    })
    .filter((s) => s.action);
}

const draftKey = (org: string, pbiId: number) => `tcm-v2-draft:${org}/${pbiId}`;

function loadDraft(org: string, pbiId: number): TestCase[] {
  try {
    const raw = localStorage.getItem(draftKey(org, pbiId));
    return raw ? (JSON.parse(raw) as TestCase[]) : [];
  } catch {
    return [];
  }
}

export default function QueuePanel({
  org,
  project,
  pbiId,
  existingTitles = [],
}: {
  org: string;
  project: string;
  pbiId: number;
  existingTitles?: string[];
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [queue, setQueue] = useState<TestCase[]>(() => loadDraft(org, pbiId));
  const [warnings, setWarnings] = useState<string[]>([]);
  const [results, setResults] = useState<SubmitItemResult[] | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  const [title, setTitle] = useState("");
  const [stepsText, setStepsText] = useState("");
  const [tags, setTags] = useState("");
  const [status, setStatus] = useState("Not Automated");

  // Draft persists per PBI so a closed app never loses queued work (v1 parity).
  useEffect(() => {
    try {
      if (queue.length === 0) localStorage.removeItem(draftKey(org, pbiId));
      else localStorage.setItem(draftKey(org, pbiId), JSON.stringify(queue));
    } catch {
      // storage unavailable -> session-only
    }
  }, [queue, org, pbiId]);

  const unlistenRef = useRef<(() => void) | null>(null);
  useEffect(() => () => unlistenRef.current?.(), []);

  function addManual() {
    const steps = parseStepsText(stepsText);
    if (!title.trim() || steps.length === 0) return;
    setQueue((q) => [
      ...q,
      {
        title: title.trim(),
        steps,
        tags: tags.trim(),
        automation_status: status,
        module_value: "",
        preconditions: "",
        update_id: null,
      },
    ]);
    setTitle("");
    setStepsText("");
    setTags("");
  }

  const importFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Import", extensions: ["xlsx", "csv", "json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.parseImportFile(path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      if (!data) return;
      setQueue((q) => [...q, ...data.cases]);
      setWarnings(data.warnings);
      toast.success(
        `Imported ${data.cases.length} case${data.cases.length === 1 ? "" : "s"}` +
          (data.warnings.length ? ` with ${data.warnings.length} warning(s)` : ""),
      );
    },
    onError: (e) => toast.error(`Import failed: ${e.message}`),
  });

  const saveTemplate = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-case-template.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path) return;
      const r = await commands.writeTemplate(path);
      if (r.status === "error") throw new Error(r.error);
      toast.success("Template saved.");
    },
    onError: (e) => toast.error(`Could not save template: ${e.message}`),
  });

  const exportQueue = useMutation({
    mutationFn: async (format: "xlsx" | "json") => {
      const path = await save({
        defaultPath: format === "xlsx" ? "test-case-queue.xlsx" : "test-case-queue.json",
        filters: [
          format === "xlsx"
            ? { name: "Excel", extensions: ["xlsx"] }
            : { name: "JSON", extensions: ["json"] },
        ],
      });
      if (!path) return;
      const r =
        format === "xlsx"
          ? await commands.exportQueue(path, queue)
          : await commands.exportQueueJson(path, queue);
      if (r.status === "error") throw new Error(r.error);
      toast.success("Queue exported.");
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  const submit = useMutation({
    mutationFn: async () => {
      setProgress({ done: 0, total: queue.length });
      unlistenRef.current = await events.submitProgress.listen((e) => {
        setProgress({ done: e.payload.index + 1, total: e.payload.total });
      });
      const r = await commands.submitQueue(
        org,
        project,
        pbiId,
        queue,
        prefs.moduleRef,
        prefs.preconditionsRef,
      );
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSettled: () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setProgress(null);
    },
    onSuccess: (data) => {
      setResults(data);
      setReviewing(false);
      const failed = new Set(data.filter((r) => r.action === "failed").map((r) => r.index));
      setQueue((q) => q.filter((_, i) => failed.has(i)));
      qc.invalidateQueries({ queryKey: ["pbi-tcs", org, pbiId] });
      const ok = data.length - failed.size;
      if (failed.size === 0) toast.success(`All ${ok} test case(s) processed.`);
      else toast.warning(`${ok} processed, ${failed.size} failed - failed items stay queued.`);
    },
    onError: (e) => toast.error(`Submit failed: ${e.message}`),
  });

  const problems = queue.map((tc) => validateCase(tc));
  const duplicates = queue.map((tc) => duplicateWarning(tc, existingTitles));
  const hasBlockers = problems.some(Boolean);

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <h2 className="text-sm font-semibold text-text">
        Queue for PBI #{pbiId} ({queue.length} queued)
      </h2>

      <div className="grid gap-2 md:grid-cols-2">
        <div className="space-y-2">
          <Input
            className="w-full"
            placeholder="Test case title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <Textarea
            className="h-24 w-full font-mono text-xs"
            placeholder={"One step per line:\naction => expected result"}
            value={stepsText}
            onChange={(e) => setStepsText(e.target.value)}
          />
          <div className="flex gap-2">
            <Input
              className="flex-1"
              placeholder="Tags (semicolon-separated)"
              value={tags}
              onChange={(e) => setTags(e.target.value)}
            />
            <Select value={status} onChange={(e) => setStatus(e.target.value)}>
              <option>Not Automated</option>
              <option>Planned</option>
            </Select>
            <Button variant="outline" size="sm" onClick={addManual}>
              Add to queue
            </Button>
          </div>
        </div>
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={importFile.isPending}
              onClick={() => importFile.mutate()}
            >
              Import file...
            </Button>
            <Button variant="outline" size="sm" onClick={() => saveTemplate.mutate()}>
              Save template...
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={queue.length === 0}
              onClick={() => exportQueue.mutate("xlsx")}
            >
              Export xlsx...
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={queue.length === 0}
              onClick={() => exportQueue.mutate("json")}
            >
              Export JSON...
            </Button>
          </div>
          {warnings.length > 0 && (
            <ul className="max-h-24 space-y-0.5 overflow-y-auto text-xs text-warning">
              {warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          )}
        </div>
      </div>

      {queue.length > 0 && (
        <ul className="space-y-1">
          {queue.map((tc, i) => (
            <li
              key={i}
              className="flex items-center justify-between rounded-md border border-border px-3 py-1.5 text-sm"
            >
              <span className="text-text">
                {tc.update_id != null && (
                  <Badge className="mr-2 bg-warning/20 text-warning">
                    UPDATE #{tc.update_id}
                  </Badge>
                )}
                {tc.title}
                <span className="ml-2 text-xs text-faint">{tc.steps.length} steps</span>
                {reviewing && problems[i] && (
                  <span className="ml-2 text-xs text-danger">{problems[i]}</span>
                )}
                {reviewing && !problems[i] && duplicates[i] && (
                  <span className="ml-2 text-xs text-warning">{duplicates[i]}</span>
                )}
              </span>
              <button
                className="text-xs text-faint hover:text-danger"
                onClick={() => setQueue((q) => q.filter((_, j) => j !== i))}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      {progress && (
        <div className="space-y-1">
          <div className="h-1.5 overflow-hidden rounded-full bg-surface-2">
            <div
              className="h-full rounded-full bg-accent transition-all"
              style={{ width: `${(progress.done / Math.max(progress.total, 1)) * 100}%` }}
            />
          </div>
          <p className="text-xs text-muted">
            Processing {progress.done}/{progress.total}...
          </p>
        </div>
      )}

      <div className="flex items-center gap-3">
        {!reviewing ? (
          <Button disabled={queue.length === 0} onClick={() => setReviewing(true)}>
            Review {queue.length} test case{queue.length === 1 ? "" : "s"}...
          </Button>
        ) : (
          <>
            <Button
              disabled={queue.length === 0 || hasBlockers || submit.isPending}
              onClick={() => submit.mutate()}
            >
              {submit.isPending ? "Creating..." : `Confirm & create ${queue.length}`}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setReviewing(false)}>
              Back
            </Button>
            {hasBlockers && (
              <span className="text-xs text-danger">Fix the flagged items first.</span>
            )}
          </>
        )}
      </div>

      {results && (
        <ul className="space-y-0.5 text-sm">
          {results.map((r) => (
            <li key={r.index} className={r.action === "failed" ? "text-danger" : "text-success"}>
              {r.action === "created" && `Created #${r.id}: ${r.title}`}
              {r.action === "updated" && `Updated #${r.id}: ${r.title}`}
              {r.action === "failed" && `Failed: ${r.title} - ${r.error}`}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
