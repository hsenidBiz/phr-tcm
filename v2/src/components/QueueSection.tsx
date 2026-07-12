import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands, events, type SubmitItemResult, type TestCase } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { unwrap } from "../lib/ipc";
import { duplicateWarning, validateCase } from "../lib/validate";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Select } from "./ui/select";

/** The shared pending-creation queue with the review gate, live progress and
 * exports. Manual Entry and Import File both render this under their own
 * input areas (v1: every tab feeds one queue). */
export default function QueueSection({
  org,
  project,
  pbiId,
  queue,
  setQueue,
}: {
  org: string;
  project: string;
  pbiId: number;
  queue: TestCase[];
  setQueue: React.Dispatch<React.SetStateAction<TestCase[]>>;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [results, setResults] = useState<SubmitItemResult[] | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [areaPath, setAreaPath] = useState("");
  const [iterationPath, setIterationPath] = useState("");

  // Classification trees load lazily, only once the review gate opens.
  const areas = useQuery({
    queryKey: ["classification", org, project, "areas"],
    queryFn: () => unwrap(commands.classificationPaths(org, project, "areas")),
    enabled: reviewing,
    staleTime: 60 * 60_000,
  });
  const iterations = useQuery({
    queryKey: ["classification", org, project, "iterations"],
    queryFn: () => unwrap(commands.classificationPaths(org, project, "iterations")),
    enabled: reviewing,
    staleTime: 60 * 60_000,
  });

  const existing = useQuery({
    queryKey: ["pbi-tc-titles", org, pbiId],
    queryFn: () => unwrap(commands.pbiTestCases(org, pbiId)),
    retry: false,
  });
  const existingTitles = (existing.data ?? []).map((t) => t.title);

  const unlistenRef = useRef<(() => void) | null>(null);
  useEffect(() => () => unlistenRef.current?.(), []);

  const exportJson = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: "test-case-queue.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const r = await commands.exportQueueJson(path, queue);
      if (r.status === "error") throw new Error(r.error);
      toast.success("Queue exported.");
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  // v1's "View": render to a temp file and open the browser - no download.
  const viewHtml = useMutation({
    mutationFn: async () => {
      const r = await commands.viewQueueHtml(queue, `PBI #${pbiId}`);
      if (r.status === "error") throw new Error(r.error);
    },
    onError: (e) => toast.error(`Could not open the report: ${e.message}`),
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
        areaPath || null,
        iterationPath || null,
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
      // Keep failed items AND anything the loop never reached (cancelled).
      const succeeded = new Set(
        data.filter((r) => r.action !== "failed").map((r) => r.index),
      );
      setQueue((q) => q.filter((_, i) => !succeeded.has(i)));
      qc.invalidateQueries({ queryKey: ["pbi-tcs", org, pbiId] });
      qc.invalidateQueries({ queryKey: ["pbi-tc-titles", org, pbiId] });
      const failedCount = data.filter((r) => r.action === "failed").length;
      const ok = succeeded.size;
      if (failedCount === 0) toast.success(`${ok} test case(s) processed.`);
      else toast.warning(`${ok} processed, ${failedCount} failed - failed items stay queued.`);
    },
    onError: (e) => toast.error(`Submit failed: ${e.message}`),
  });

  const problems = queue.map((tc) => validateCase(tc));
  const duplicates = queue.map((tc) => duplicateWarning(tc, existingTitles));
  const hasBlockers = problems.some(Boolean);

  return (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-text">
          Queue for PBI #{pbiId} ({queue.length} queued)
        </h2>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || viewHtml.isPending}
            onClick={() => viewHtml.mutate()}
          >
            View in browser
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0}
            onClick={() => exportJson.mutate()}
          >
            Export JSON...
          </Button>
        </div>
      </div>

      {queue.length === 0 && (
        <p className="text-sm text-muted">Nothing queued yet - add cases above.</p>
      )}

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
          <div className="flex items-center justify-between">
            <p className="text-xs text-muted">
              Processing {progress.done}/{progress.total}...
            </p>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                commands.cancelSubmit();
                toast.info("Stopping after the current item...");
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}

      {reviewing && (
        <div className="flex flex-wrap gap-3">
          <label className="flex flex-col gap-1 text-xs text-muted">
            Area path for new cases
            <Select
              className="w-64 py-1.5"
              value={areaPath}
              onChange={(e) => setAreaPath(e.target.value)}
            >
              <option value="">Same as PBI</option>
              {(areas.data ?? []).map((p) => (
                <option key={p}>{p}</option>
              ))}
            </Select>
          </label>
          <label className="flex flex-col gap-1 text-xs text-muted">
            Iteration for new cases
            <Select
              className="w-64 py-1.5"
              value={iterationPath}
              onChange={(e) => setIterationPath(e.target.value)}
            >
              <option value="">Same as PBI</option>
              {(iterations.data ?? []).map((p) => (
                <option key={p}>{p}</option>
              ))}
            </Select>
          </label>
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
