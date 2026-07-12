import { useMutation, useQuery } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Camera, ClipboardPaste, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Toaster, toast } from "sonner";
import { commands, type TestCaseFull } from "../bindings";
import BugDialog from "../components/BugDialog";
import { Button } from "../components/ui/button";
import { Textarea } from "../components/ui/input";
import { cn } from "../lib/cn";
import { unwrap, unwrapStr } from "../lib/ipc";
import { loadRunnerSession } from "../lib/runnerSession";
import { getTheme } from "../lib/theme";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

type CaseState = {
  outcome: Outcome;
  comment: string;
  stepOutcomes: Record<number, Outcome>;
  screenshots: string[]; // b64 png
  elapsedMs: number;
  bugIds: number[];
};

function emptyState(): CaseState {
  return { outcome: "", comment: "", stepOutcomes: {}, screenshots: [], elapsedMs: 0, bugIds: [] };
}

const outcomeBtn: Record<string, string> = {
  Passed: "bg-success text-on-accent",
  Failed: "bg-danger text-on-accent",
  Blocked: "bg-warning text-on-accent",
  NotApplicable: "bg-surface-2 text-muted",
};

export default function RunnerWindow() {
  const session = loadRunnerSession();
  const [idx, setIdx] = useState(0);
  const [states, setStates] = useState<Record<number, CaseState>>({});
  const [bugFor, setBugFor] = useState<TestCaseFull | null>(null);
  const startRef = useRef<number>(Date.now());

  const cases = useQuery({
    queryKey: ["runner-cases", session?.org, session?.pbi.id],
    queryFn: () => unwrap(commands.pbiTestCasesFull(session!.org, session!.pbi.id, null, null)),
    enabled: Boolean(session),
    retry: false,
  });

  // Points load up front so each case can preload its last comment and
  // offer its previously-uploaded screenshots (v1 lazy-preload parity).
  const points = useQuery({
    queryKey: ["runner-points", session?.org, session?.planId, session?.suiteId],
    queryFn: () =>
      unwrap(
        commands.listTestPoints(session!.org, session!.project, session!.planId, session!.suiteId),
      ),
    enabled: Boolean(session),
    retry: false,
  });

  const list = cases.data ?? [];
  const current = list[idx];
  const st = (current && states[current.id]) || emptyState();
  const currentPoint = points.data?.find((p) => p.test_case_id === current?.id);
  const [uploaded, setUploaded] = useState<Record<number, string[]>>({});

  // Preload the last run's comment once per case, only while untouched.
  useEffect(() => {
    if (!current || !currentPoint?.last_run_id || !currentPoint.last_result_id) return;
    if (states[current.id]?.comment) return;
    let stale = false;
    unwrap(
      commands.getResultDetail(
        session!.org,
        session!.project,
        currentPoint.last_run_id,
        currentPoint.last_result_id,
      ),
    )
      .then((d) => {
        if (!stale && d.comment && !states[current.id]?.comment) {
          patch(current.id, { comment: d.comment });
        }
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, currentPoint?.last_result_id]);

  const loadUploaded = useMutation({
    mutationFn: () =>
      unwrap(
        commands.resultScreenshots(
          session!.org,
          session!.project,
          currentPoint!.last_run_id!,
          currentPoint!.last_result_id!,
        ),
      ),
    onSuccess: (shots) => {
      if (current) setUploaded((u) => ({ ...u, [current.id]: shots }));
      if (shots.length === 0) toast.info("No screenshots on the last result.");
    },
    onError: (e) => toast.error(`Could not load screenshots: ${e.message}`),
  });

  // Per-case timer -> duration_ms. Reset on case switch.
  useEffect(() => {
    startRef.current = Date.now();
  }, [idx]);

  const patch = (caseId: number, p: Partial<CaseState>) =>
    setStates((s) => ({ ...s, [caseId]: { ...emptyState(), ...s[caseId], ...p } }));

  const capture = useMutation({
    mutationFn: () => unwrapStr(commands.captureScreens()),
    onSuccess: (shots) => {
      if (!current) return;
      patch(current.id, {
        screenshots: [...st.screenshots, ...shots.map((s) => s.b64_png)],
      });
      toast.success(`Captured ${shots.length} screen(s)`);
    },
    onError: (e) => toast.error(`Capture failed: ${e.message}`),
  });

  async function pasteImage() {
    if (!current) return;
    try {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const type = item.types.find((t) => t.startsWith("image/"));
        if (!type) continue;
        const blob = await item.getType(type);
        const b64 = await blobToB64(blob);
        patch(current.id, { screenshots: [...st.screenshots, b64] });
        toast.success("Pasted screenshot");
        return;
      }
      toast.info("No image on the clipboard.");
    } catch {
      toast.error("Clipboard image paste is not available.");
    }
  }

  const finish = useMutation({
    mutationFn: async () => {
      const outcomes = list
        .map((c) => ({ c, s: states[c.id] }))
        .filter(({ s }) => s?.outcome)
        .map(({ c, s }) => {
          const marked = Object.keys(s.stepOutcomes);
          return {
            point_id: 0, // filled below once points resolve
            case: c,
            outcome: s.outcome as string,
            comment: s.comment || null,
            duration_ms: s.elapsedMs || null,
            step_ids: marked.length ? c.step_ids : null,
            step_outcomes: marked.length
              ? c.steps.map((_, i) => s.stepOutcomes[i] || null)
              : null,
            attachments: s.screenshots.length
              ? s.screenshots.map((b64, i) => ({
                  file_name: `screenshot-${c.id}-${i + 1}.png`,
                  b64,
                }))
              : null,
            bug_ids: s.bugIds.length ? s.bugIds : null,
          };
        });
      if (outcomes.length === 0) throw new Error("Mark at least one case first.");

      // Resolve point ids for the chosen cases.
      const points = await unwrap(
        commands.listTestPoints(session!.org, session!.project, session!.planId, session!.suiteId),
      );
      const byCase = new Map(points.map((p) => [p.test_case_id, p.point_id]));
      const resolved = outcomes
        .map((o) => ({ ...o, point_id: byCase.get(o.case.id) ?? 0 }))
        .filter((o) => o.point_id !== 0)
        .map(({ case: _c, ...rest }) => rest);
      if (resolved.length === 0) throw new Error("None of the marked cases have a test point.");

      return unwrap(
        commands.submitTestRun(
          session!.org,
          session!.project,
          session!.planId,
          `${session!.pbi.title} - manual run`,
          resolved,
        ),
      );
    },
    onSuccess: () => {
      toast.success("Run recorded. Closing runner.");
      setTimeout(() => getCurrentWindow().close(), 600);
    },
    onError: (e) => toast.error(e.message),
  });

  if (!session) {
    return <div className="p-6 text-sm text-muted">No run session. Open the runner from Run Tests.</div>;
  }

  const markedCount = Object.values(states).filter((s) => s.outcome).length;

  return (
    <div className="flex h-screen flex-col bg-bg text-text">
      <Toaster theme={getTheme() === "light" ? "light" : "dark"} richColors position="bottom-right" />
      <header
        data-tauri-drag-region
        className="flex select-none items-center gap-2 border-b border-border bg-surface px-3 py-2"
      >
        <span className="pointer-events-none text-sm font-semibold">Runner</span>
        <span className="id-mono text-xs text-faint">#{session.pbi.id}</span>
        <span className="ml-auto text-xs text-muted">
          {markedCount}/{list.length} marked
        </span>
        <button
          aria-label="Close runner"
          className="rounded p-1 text-muted hover:text-danger"
          onClick={() => getCurrentWindow().close()}
        >
          <X size={15} />
        </button>
      </header>

      {cases.isLoading && <p className="p-4 text-sm text-muted">Loading test cases...</p>}
      {cases.isError && <p className="p-4 text-sm text-danger">{cases.error.message}</p>}
      {cases.data && list.length === 0 && (
        <p className="p-4 text-sm text-muted">No linked test cases to run.</p>
      )}

      {current && (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
          <div>
            <div className="text-xs text-faint">
              Case {idx + 1} of {list.length}
            </div>
            <div className="font-medium">{current.title}</div>
          </div>

          <ol className="space-y-1.5">
            {current.steps.map((step, i) => (
              <li key={i} className="rounded-md border border-border p-2 text-sm">
                <div className="flex items-start gap-2">
                  <span className="id-mono text-xs text-faint">{i + 1}</span>
                  <div className="flex-1">
                    <div>{step.action}</div>
                    {step.expected && (
                      <div className="text-xs text-muted">Expect: {step.expected}</div>
                    )}
                  </div>
                  <div className="flex gap-1">
                    {(["Passed", "Failed"] as const).map((o) => (
                      <button
                        key={o}
                        title={o}
                        className={cn(
                          "rounded px-1.5 py-0.5 text-[10px] font-semibold",
                          st.stepOutcomes[i] === o ? outcomeBtn[o] : "bg-surface-2 text-muted",
                        )}
                        onClick={() =>
                          patch(current.id, {
                            stepOutcomes: { ...st.stepOutcomes, [i]: o },
                          })
                        }
                      >
                        {o[0]}
                      </button>
                    ))}
                  </div>
                </div>
              </li>
            ))}
          </ol>

          <Textarea
            aria-label="Comment"
            className="h-16 w-full text-sm"
            placeholder="Comment (optional)"
            value={st.comment}
            onChange={(e) => patch(current.id, { comment: e.target.value })}
          />

          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" size="sm" disabled={capture.isPending} onClick={() => capture.mutate()}>
              <Camera size={14} /> Capture
            </Button>
            <Button variant="outline" size="sm" onClick={pasteImage}>
              <ClipboardPaste size={14} /> Paste
            </Button>
            {st.screenshots.length > 0 && (
              <span className="text-xs text-muted">{st.screenshots.length} shot(s)</span>
            )}
            {(st.outcome === "Failed" || Object.values(st.stepOutcomes).includes("Failed")) && (
              <Button variant="outline" size="sm" onClick={() => setBugFor(current)}>
                File bug
              </Button>
            )}
            {st.bugIds.length > 0 && (
              <span className="text-xs text-success">bug #{st.bugIds.join(", #")}</span>
            )}
            {currentPoint?.last_run_id && currentPoint.last_result_id && !uploaded[current.id] && (
              <Button
                variant="ghost"
                size="sm"
                disabled={loadUploaded.isPending}
                onClick={() => loadUploaded.mutate()}
              >
                {loadUploaded.isPending ? "Loading..." : "View uploaded"}
              </Button>
            )}
          </div>

          {(uploaded[current.id]?.length ?? 0) > 0 && (
            <div className="space-y-1">
              <div className="text-xs text-muted">Previously uploaded:</div>
              <div className="flex flex-wrap gap-1">
                {uploaded[current.id].map((b64, i) => (
                  <img
                    key={i}
                    alt={`Uploaded screenshot ${i + 1}`}
                    className="h-16 rounded border border-border object-cover"
                    src={`data:image/png;base64,${b64}`}
                  />
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-1">
            {OUTCOMES.map((o) => (
              <button
                key={o}
                className={cn(
                  "flex-1 rounded-md py-1.5 text-xs font-semibold",
                  st.outcome === o ? outcomeBtn[o] : "border border-border text-muted hover:text-text",
                )}
                onClick={() =>
                  patch(current.id, { outcome: o, elapsedMs: Date.now() - startRef.current })
                }
              >
                {o}
              </button>
            ))}
          </div>
        </div>
      )}

      <footer className="flex items-center gap-2 border-t border-border bg-surface px-3 py-2">
        <Button variant="ghost" size="sm" disabled={idx === 0} onClick={() => setIdx((i) => i - 1)}>
          Prev
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={idx >= list.length - 1}
          onClick={() => setIdx((i) => i + 1)}
        >
          Next
        </Button>
        <Button
          className="ml-auto"
          size="sm"
          disabled={markedCount === 0 || finish.isPending}
          onClick={() => finish.mutate()}
        >
          {finish.isPending ? "Recording..." : `Finish (${markedCount})`}
        </Button>
      </footer>

      {bugFor && (
        <BugDialog
          org={session.org}
          project={session.project}
          testCase={bugFor}
          pbiId={session.pbi.id}
          screenshots={states[bugFor.id]?.screenshots ?? []}
          onClose={() => setBugFor(null)}
          onFiled={(bugId) => {
            patch(bugFor.id, { bugIds: [...(states[bugFor.id]?.bugIds ?? []), bugId] });
            setBugFor(null);
          }}
        />
      )}
    </div>
  );
}

function blobToB64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = reject;
    reader.readAsDataURL(blob);
  });
}
