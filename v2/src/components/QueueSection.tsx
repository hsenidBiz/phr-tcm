import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { commands, events, type SubmitItemResult, type TestCase } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { diffCase, diffSummary } from "../lib/caseDiff";
import { loadNotes } from "../lib/caseNotes";
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

  // Diff-preview (spec EDT-B): once the review gate opens, fetch the
  // current server values for every queued UPDATE in one batch so rows
  // can show what will actually change. Failure degrades to "diff
  // unavailable" - it never blocks submitting.
  const updateIds = queue
    .map((tc) => tc.update_id)
    .filter((x): x is number => x != null);
  const currentCases = useQuery({
    queryKey: ["diff-cases", org, [...updateIds].sort(), prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(commands.testCasesByIds(org, updateIds, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: reviewing && updateIds.length > 0,
    staleTime: 60_000,
    retry: false,
  });
  const currentById = new Map((currentCases.data ?? []).map((c) => [c.id, c]));
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());
  const toggleDiff = (i: number) =>
    setExpandedDiffs((s) => {
      const next = new Set(s);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });

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
      const r = await commands.viewQueueHtml(queue, `PBI #${pbiId}`, org, loadNotes(org));
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
            Export JSON
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={queue.length === 0 || submit.isPending}
            onClick={() => {
              const n = queue.length;
              setQueue([]);
              toast.info(`Removed ${n} queued case${n === 1 ? "" : "s"}.`);
            }}
          >
            Remove all
          </Button>
        </div>
      </div>

      {queue.length === 0 && (
        <p className="text-sm text-muted">Nothing queued yet - add cases above.</p>
      )}

      {queue.length > 0 && (
        <ul className="space-y-1">
          {queue.map((tc, i) => {
            const cur = tc.update_id != null ? currentById.get(tc.update_id) : undefined;
            const diff = reviewing && cur ? diffCase(tc, cur) : null;
            const diffFailed =
              reviewing && tc.update_id != null && !cur && currentCases.isError;
            return (
              <li key={i} className="rounded-md border border-border text-sm">
                <div className="flex items-center justify-between px-3 py-1.5">
                  <span className="text-text">
                    {tc.update_id != null ? (
                      <Badge className="mr-2 bg-warning/20 text-warning">
                        UPDATE #{tc.update_id}
                      </Badge>
                    ) : (
                      <Badge className="mr-2 bg-success/20 text-success">NEW</Badge>
                    )}
                    {tc.title}
                    <span className="ml-2 text-xs text-faint">{tc.steps.length} steps</span>
                    {diff?.noop && (
                      <Badge className="ml-2 bg-warning/20 text-warning">
                        no-op — nothing will change
                      </Badge>
                    )}
                    {diff && !diff.noop && (
                      <button
                        className="ml-2 text-xs text-accent hover:underline"
                        onClick={() => toggleDiff(i)}
                      >
                        {diffSummary(diff)} {expandedDiffs.has(i) ? "▾" : "▸"}
                      </button>
                    )}
                    {diffFailed && (
                      <span className="ml-2 text-xs text-faint">diff unavailable</span>
                    )}
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
                </div>
                {diff && !diff.noop && expandedDiffs.has(i) && (
                  <div className="space-y-1 border-t border-border px-3 py-2 text-xs">
                    {diff.fields.map((f) => (
                      <div key={f.name}>
                        <span className="font-medium text-muted">{f.name}:</span>{" "}
                        <span className="text-danger line-through">{f.old || "(empty)"}</span>{" "}
                        <span className="text-faint">→</span>{" "}
                        <span className="text-success">{f.new}</span>
                      </div>
                    ))}
                    {diff.steps.detail.length > 0 && (
                      <div className="space-y-1">
                        <span className="font-medium text-muted">Steps:</span>
                        {/* git-style: the server's line as "-", the queued
                            line as "+", so before/after reads at a glance. */}
                        {diff.steps.detail.map((d) => (
                          <div key={d.index} className="space-y-0.5">
                            {d.old && (
                              <div className="flex gap-2 rounded bg-danger/10 px-2 py-1 text-danger">
                                <span className="select-none font-semibold">-</span>
                                <span className="whitespace-pre-wrap">
                                  <span className="id-mono opacity-70">#{d.index + 1}</span>{" "}
                                  {d.old.action}
                                  {d.old.expected && (
                                    <span className="opacity-80"> ⇒ {d.old.expected}</span>
                                  )}
                                </span>
                              </div>
                            )}
                            {d.new && (
                              <div className="flex gap-2 rounded bg-success/10 px-2 py-1 text-success">
                                <span className="select-none font-semibold">+</span>
                                <span className="whitespace-pre-wrap">
                                  <span className="id-mono opacity-70">#{d.index + 1}</span>{" "}
                                  {d.new.action}
                                  {d.new.expected && (
                                    <span className="opacity-80"> ⇒ {d.new.expected}</span>
                                  )}
                                </span>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                    {diff.blankSkipped.length > 0 && (
                      <div className="text-faint">
                        Left untouched (blank in import): {diff.blankSkipped.join(", ")}
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
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
              Processing {progress.done}/{progress.total}
            </p>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                commands.cancelSubmit();
                toast.info("Stopping after the current item");
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
            Review {queue.length} test case{queue.length === 1 ? "" : "s"}
          </Button>
        ) : (
          <>
            <Button
              disabled={queue.length === 0 || hasBlockers || submit.isPending}
              onClick={() => submit.mutate()}
            >
              {submit.isPending ? "Creating" : `Confirm & create ${queue.length}`}
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
