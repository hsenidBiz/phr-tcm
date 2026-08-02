import { useLightbox } from "@astryxdesign/core/Lightbox";
import { useMutation, useQuery } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { Pin, PinOff, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Toaster, toast } from "sonner";
import { commands, type RunAttachment, type TestCaseFull } from "../bindings";
import AstryxIsland from "../components/AstryxIsland";
import BugDialog from "../components/BugDialog";
import HistoryDots from "../components/HistoryDots";
import { Button } from "../components/ui/button";
import { Textarea } from "../components/ui/input";
import { cn } from "../lib/cn";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { unwrap, unwrapStr } from "../lib/ipc";
import { loadRunnerPinned, loadRunnerSession, saveRunnerPinned } from "../lib/runnerSession";
import { getTheme } from "../lib/theme";
import { outcomeLabel } from "./RunPanel";
import {
  IconAttach,
  IconBack,
  IconBug,
  IconFinish,
  IconNext,
  IconPasteImage,
  IconRecord,
  IconSnip,
} from "../lib/actionIcons";

const OUTCOMES = ["Passed", "Failed", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

type CaseState = {
  outcome: Outcome;
  comment: string;
  stepOutcomes: Record<number, Outcome>;
  attachments: RunAttachment[];
  elapsedMs: number;
  bugIds: number[];
};

function emptyState(): CaseState {
  return { outcome: "", comment: "", stepOutcomes: {}, attachments: [], elapsedMs: 0, bugIds: [] };
}

const outcomeBtn: Record<string, string> = {
  Passed: "bg-success text-on-accent",
  Failed: "bg-danger text-on-accent",
  Blocked: "bg-warning text-on-accent",
  NotApplicable: "bg-surface-2 text-muted",
};

const outcomeBadge: Record<string, string> = {
  passed: "bg-success/15 text-success",
  failed: "bg-danger/15 text-danger",
  blocked: "bg-warning/15 text-warning",
  notapplicable: "bg-surface-2 text-muted",
};

const IMAGE_EXT = /\.(png|jpe?g|gif|bmp|webp)$/i;

async function readClipboardImageB64(): Promise<string | null> {
  const items = await navigator.clipboard.read();
  for (const item of items) {
    const type = item.types.find((t) => t.startsWith("image/"));
    if (!type) continue;
    const blob = await item.getType(type);
    return blobToB64(blob);
  }
  return null;
}

export default function RunnerWindow() {
  const session = loadRunnerSession();
  const [idx, setIdx] = useState(0);
  const [states, setStates] = useState<Record<number, CaseState>>({});
  const [bugFor, setBugFor] = useState<TestCaseFull | null>(null);
  // The window is created with the remembered pin preference (openRunner
  // reads the same key), so state and reality start in sync.
  const [pinned, setPinned] = useState(loadRunnerPinned);
  const [snipping, setSnipping] = useState(false);
  const [recording, setRecording] = useState(false);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const startRef = useRef<number>(Date.now());
  const snipToken = useRef(0);

  // Preconditions (and Module) live in org-specific CUSTOM fields - without
  // their reference names the fetch returns them empty, which is why the
  // preconditions block never showed. Same detection as the main window
  // (shared localStorage cache, auto-picked from the field list otherwise).
  const { prefs } = useFieldRefs(session?.org ?? "", session?.project ?? "");

  const cases = useQuery({
    queryKey: [
      "runner-cases",
      session?.org,
      session?.pbi.id,
      prefs.moduleRef,
      prefs.preconditionsRef,
    ],
    queryFn: () =>
      unwrap(
        commands.pbiTestCasesFull(
          session!.org,
          session!.pbi.id,
          prefs.moduleRef,
          prefs.preconditionsRef,
        ),
      ),
    enabled: Boolean(session),
    retry: false,
  });

  // Points load up front so each case can show its existing status and
  // preload the last run's comment + screenshots.
  const points = useQuery({
    queryKey: ["runner-points", session?.org, session?.planId, session?.suiteId],
    queryFn: () =>
      unwrap(
        commands.listTestPoints(session!.org, session!.project, session!.planId, session!.suiteId),
      ),
    enabled: Boolean(session),
    retry: false,
  });

  // Last-5 outcome history (shared shape with Run Tests, own QueryClient).
  const history = useQuery({
    queryKey: ["run-history", session?.org, session?.project, session?.planId],
    queryFn: () => unwrap(commands.runHistory(session!.org, session!.project, session!.planId)),
    enabled: Boolean(session),
    staleTime: 5 * 60_000,
    retry: false,
  });

  const caseFilter = session?.caseIds?.length ? new Set(session.caseIds) : null;
  const list = (cases.data ?? []).filter((c) => !caseFilter || caseFilter.has(c.id));
  const current = list[idx];
  const st = (current && states[current.id]) || emptyState();
  const currentPoint = points.data?.find((p) => p.test_case_id === current?.id);

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
        if (stale || !d.comment) return;
        // Decided against the state as it is NOW. The `states` the effect
        // closed over is from the render that started this request, so
        // anything the tester typed while it was in flight was invisible
        // here - and the old run's comment landed on top of it.
        setStates((s) =>
          s[current.id]?.comment
            ? s
            : { ...s, [current.id]: { ...emptyState(), ...s[current.id], comment: d.comment } },
        );
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, currentPoint?.last_result_id]);

  // Previously-uploaded screenshots load automatically when a case with a
  // prior result becomes current (v1 "auto-load" request; no button).
  const uploaded = useQuery({
    queryKey: [
      "uploaded-shots",
      session?.org,
      currentPoint?.last_run_id,
      currentPoint?.last_result_id,
    ],
    queryFn: () =>
      unwrap(
        commands.resultScreenshots(
          session!.org,
          session!.project,
          currentPoint!.last_run_id!,
          currentPoint!.last_result_id!,
        ),
      ),
    enabled: Boolean(session && currentPoint?.last_run_id && currentPoint?.last_result_id),
    staleTime: Infinity,
    retry: false,
  });

  // Fullscreen viewer for the uploaded-screenshot thumbnails (zoom/pan +
  // prev/next). Media rebuilds with the query, so triggers stay in sync.
  const shots = (uploaded.data ?? []).map((b64, i) => ({
    src: `data:image/png;base64,${b64}`,
    alt: `Uploaded screenshot ${i + 1}`,
  }));
  const lightbox = useLightbox({ media: shots });

  // Per-case timer -> duration_ms. Reset on case switch.
  useEffect(() => {
    startRef.current = Date.now();
  }, [idx]);

  // Every case opens with its LAST outcome already selected. A tester
  // re-running a suite only touches what changed: a case that passed last
  // time and passed again needs no click at all, and the lit button is
  // itself the "this failed last time" indicator while re-testing.
  //
  // Pre-selected marks count toward Finish exactly like clicked ones - that
  // is the point - and the footer's "N/M marked" plus "Finish (N)" say how
  // many will be recorded before anything is sent. Never-run cases stay
  // unmarked, and a mark the tester has already made is never overwritten,
  // so a refetch of points mid-session cannot undo a decision.
  useEffect(() => {
    if (!points.data || !cases.data) return;
    setStates((s) => {
      let changed = false;
      const next = { ...s };
      for (const p of points.data) {
        if (p.test_case_id == null || next[p.test_case_id]?.outcome) continue;
        const last = OUTCOMES.find((o) => o.toLowerCase() === p.last_outcome.toLowerCase());
        if (!last) continue; // never run - a blank slate stays blank
        next[p.test_case_id] = { ...emptyState(), ...next[p.test_case_id], outcome: last };
        changed = true;
      }
      return changed ? next : s;
    });
  }, [points.data, cases.data]);

  const patch = (caseId: number, p: Partial<CaseState>) =>
    setStates((s) => ({ ...s, [caseId]: { ...emptyState(), ...s[caseId], ...p } }));

  const addImage = (caseId: number, b64: string, kind: string) =>
    setStates((s) => {
      const prev = s[caseId] ?? emptyState();
      const att = { file_name: `${kind}-${caseId}-${prev.attachments.length + 1}.png`, b64 };
      return { ...s, [caseId]: { ...prev, attachments: [...prev.attachments, att] } };
    });

  const addAttachment = (caseId: number, att: RunAttachment) =>
    setStates((s) => {
      const prev = s[caseId] ?? emptyState();
      return { ...s, [caseId]: { ...prev, attachments: [...prev.attachments, att] } };
    });

  const removeAttachment = (caseId: number, index: number) =>
    setStates((s) => {
      const prev = s[caseId] ?? emptyState();
      return {
        ...s,
        [caseId]: { ...prev, attachments: prev.attachments.filter((_, i) => i !== index) },
      };
    });

  /** Screen recording, like Azure DevOps's own runner: pick a screen or
   * window, record, and the .webm lands as an attachment on THIS case's
   * result (captured at start, so switching cases mid-recording still
   * files it correctly). Stopping works from our button or the browser's
   * own "stop sharing" bar. */
  async function toggleRecord() {
    if (recorderRef.current) {
      recorderRef.current.stop();
      return;
    }
    if (!current) return;
    const caseId = current.id;
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({ video: true, audio: false });
      const rec = MediaRecorder.isTypeSupported?.("video/webm")
        ? new MediaRecorder(stream, { mimeType: "video/webm" })
        : new MediaRecorder(stream);
      const chunks: Blob[] = [];
      rec.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data);
      };
      rec.onstop = async () => {
        recorderRef.current = null;
        setRecording(false);
        stream.getTracks().forEach((t) => t.stop());
        const blob = new Blob(chunks, { type: "video/webm" });
        if (blob.size === 0) {
          toast.info("Recording was empty - nothing attached.");
          return;
        }
        const b64 = await blobToB64(blob);
        addAttachment(caseId, { file_name: `recording-${caseId}-${Date.now()}.webm`, b64 });
        toast.success("Recording attached");
      };
      stream.getVideoTracks()[0]?.addEventListener("ended", () => {
        if (rec.state !== "inactive") rec.stop();
      });
      rec.start();
      recorderRef.current = rec;
      setRecording(true);
    } catch {
      toast.error(
        "Screen recording unavailable - record with Win+Alt+R (Game Bar) and use Attach file.",
      );
    }
  }

  const togglePin = () => {
    const next = !pinned;
    setPinned(next);
    saveRunnerPinned(next); // the next run opens the way you left it
    getCurrentWindow()
      .setAlwaysOnTop(next)
      .catch(() => toast.error("Could not change always-on-top."));
  };

  /** Region capture via the Windows snip overlay: the result lands on the
   * clipboard, so poll for a NEW image for up to 60s and auto-attach it. */
  async function snip() {
    if (!current) return;
    const caseId = current.id;
    const before = await readClipboardImageB64().catch(() => null);
    const r = await commands.openSnip();
    if (r.status === "error") {
      toast.error(`Could not open the snipping overlay: ${r.error}. Snip manually and use Paste.`);
      return;
    }
    const token = ++snipToken.current;
    setSnipping(true);
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline && snipToken.current === token) {
      await new Promise((res) => setTimeout(res, 1000));
      const img = await readClipboardImageB64().catch(() => null);
      if (img && img !== before) {
        addImage(caseId, img, "snip");
        toast.success("Snip attached");
        setSnipping(false);
        return;
      }
    }
    if (snipToken.current === token) {
      setSnipping(false);
      toast.info("No snip detected - use Paste if you captured one.");
    }
  }

  async function pasteImage() {
    if (!current) return;
    try {
      const b64 = await readClipboardImageB64();
      if (b64) {
        addImage(current.id, b64, "pasted");
        toast.success("Pasted screenshot");
      } else {
        toast.info("No image on the clipboard.");
      }
    } catch {
      toast.error("Clipboard image paste is not available.");
    }
  }

  async function attachFile() {
    if (!current) return;
    const path = await open({ multiple: false, title: "Attach a file to this result" });
    if (typeof path !== "string") return;
    try {
      const att = await unwrapStr(commands.readFileB64(path));
      addAttachment(current.id, att);
      toast.success(`Attached ${att.file_name}`);
    } catch (e) {
      toast.error(`Could not read the file: ${(e as Error).message}`);
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
            attachments: s.attachments.length ? s.attachments : null,
            bug_ids: s.bugIds.length ? s.bugIds : null,
          };
        });
      if (outcomes.length === 0) throw new Error("Mark at least one case first.");

      // Resolve point ids for the chosen cases.
      const pts = await unwrap(
        commands.listTestPoints(session!.org, session!.project, session!.planId, session!.suiteId),
      );
      const byCase = new Map(pts.map((p) => [p.test_case_id, p.point_id]));
      // Membership, not a `?? 0` sentinel: a real point id of 0 would
      // otherwise be indistinguishable from "this case has no point".
      const dropped = outcomes.filter((o) => !byCase.has(o.case.id)).map((o) => o.case.title);
      const resolved = outcomes
        .filter((o) => byCase.has(o.case.id))
        .map(({ case: c, ...rest }) => ({ ...rest, point_id: byCase.get(c.id)! }));
      if (resolved.length === 0) throw new Error("None of the marked cases have a test point.");

      const run = await unwrap(
        commands.submitTestRun(
          session!.org,
          session!.project,
          session!.planId,
          `${session!.pbi.title} - manual run`,
          resolved,
        ),
      );
      // A PARTIAL drop used to pass silently: only an all-dropped run
      // errored, so a tester who marked eight cases and had two without a
      // point was told the run was recorded and lost those two results.
      return {
        recorded: resolved.length - run.outcomes_unrecorded.length,
        dropped,
        unrecorded: run.outcomes_unrecorded,
        extrasFailed: run.extras_failed,
      };
    },
    onSuccess: ({ recorded, dropped, unrecorded, extrasFailed }) => {
      // Each message is guarded by the list it describes. They were briefly
      // chained, with the last one reached by FALL-THROUGH - so an
      // unrecorded-only run printed the suite message too, with
      // "0 could not be:" and no names, contradicting the accurate warning
      // just above it. A message must never be able to fire about a list
      // that is empty.

      // Marked, but Azure DevOps had no result row: NOT recorded, and not
      // something that can be added over there. It has to be marked again
      // in here - which is the opposite of the attachment message below.
      if (unrecorded.length > 0) {
        toast.warning(
          `${recorded} result(s) recorded. ${unrecorded.length} could NOT be - Azure DevOps ` +
            `created no result row for test point(s) ${unrecorded.join(", ")}. Mark those ` +
            `cases again here; they are not in the run.`,
          { duration: 20000 },
        );
      }

      // Not in the suite at all, so there was never a test point for them.
      if (dropped.length > 0) {
        const one = dropped.length === 1;
        toast.warning(
          `${recorded} result(s) recorded, but ${dropped.length} could not be: ` +
            `${dropped.join(", ")} - ${one ? "it is" : "they are"} not in this suite, so there ` +
            `is no test point to record against. Add ${one ? "it" : "them"} to the suite and ` +
            `mark again.`,
          { duration: 20000 },
        );
      }

      // Attached AFTER the outcomes are saved, so these genuinely are
      // "recorded, but this did not attach".
      if (extrasFailed.length > 0) {
        toast.warning(
          `The outcomes were recorded, but this did not attach: ${extrasFailed.join(", ")}. ` +
            `Add it in Azure DevOps.`,
          { duration: 20000 },
        );
      }

      // Only a completely clean run closes the window. Anything else leaves
      // it open so the tester can read what happened - and the Finish
      // button is latched, so it cannot be sent a second time.
      if (dropped.length === 0 && unrecorded.length === 0 && extrasFailed.length === 0) {
        toast.success("Run recorded. Closing runner.");
        setTimeout(() => getCurrentWindow().close(), 600);
      }
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
        {caseFilter && (
          <span className="rounded bg-accent-soft px-1.5 text-[11px] text-accent">
            {caseFilter.size} selected
          </span>
        )}
        <span className="ml-auto text-xs text-muted">
          {markedCount}/{list.length} marked
        </span>
        <button
          aria-label={pinned ? "Unpin (allow other windows on top)" : "Pin on top"}
          title={pinned ? "Unpin (allow other windows on top)" : "Pin on top"}
          className={cn("rounded p-1", pinned ? "text-accent" : "text-muted hover:text-text")}
          onClick={togglePin}
        >
          {pinned ? <Pin size={14} /> : <PinOff size={14} />}
        </button>
        <button
          aria-label="Close runner"
          className="rounded p-1 text-muted hover:text-danger"
          onClick={() => getCurrentWindow().close()}
        >
          <X size={15} />
        </button>
      </header>

      {cases.isLoading && <p className="p-4 text-sm text-muted">Loading test cases</p>}
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
            <div className="flex items-center gap-2">
              <span className="font-medium">{current.title}</span>
              {currentPoint?.last_outcome && (
                <span
                  className={cn(
                    "rounded px-1.5 py-0.5 text-[11px] font-medium",
                    outcomeBadge[currentPoint.last_outcome.toLowerCase()] ?? "bg-surface-2 text-muted",
                  )}
                >
                  Last: {outcomeLabel(currentPoint.last_outcome)}
                </span>
              )}
              {(history.data?.find((h) => h.test_case_id === current.id)?.outcomes.length ?? 0) >
                0 && (
                <HistoryDots
                  size={7}
                  outcomes={history.data!.find((h) => h.test_case_id === current.id)!.outcomes}
                />
              )}
            </div>
          </div>

          {current.preconditions.trim() !== "" && (
            <div className="rounded-md border border-accent/40 bg-accent-soft/30 p-2 text-sm">
              <div className="mb-0.5 text-xs font-semibold text-accent">Preconditions</div>
              <div className="whitespace-pre-wrap text-text">{current.preconditions}</div>
            </div>
          )}

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
                          "pill-label rounded px-1.5 text-[10px] font-semibold",
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
            <Button
              variant={recording ? "danger" : "outline"}
              size="sm"
              onClick={toggleRecord}
              title="Record the screen; the video attaches to this result"
            >
              <IconRecord aria-hidden />{recording ? "Stop recording" : "Record"}
            </Button>
            <Button variant="outline" size="sm" disabled={snipping} onClick={snip}>
              <IconSnip aria-hidden />{snipping ? "Waiting for snip" : "Snip"}
            </Button>
            <Button variant="outline" size="sm" onClick={pasteImage}>
              <IconPasteImage aria-hidden />Paste
            </Button>
            <Button variant="outline" size="sm" onClick={attachFile}>
              <IconAttach aria-hidden />Attach file
            </Button>
            {(st.outcome === "Failed" || Object.values(st.stepOutcomes).includes("Failed")) && (
              <Button variant="outline" size="sm" onClick={() => setBugFor(current)}>
                <IconBug aria-hidden />
                File bug
              </Button>
            )}
            {st.bugIds.length > 0 && (
              <span className="text-xs text-success">bug #{st.bugIds.join(", #")}</span>
            )}
          </div>

          {st.attachments.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {st.attachments.map((a, i) => (
                <span
                  key={i}
                  className="flex items-center gap-1 rounded bg-surface-2 px-1.5 py-0.5 text-[11px] text-muted"
                >
                  {a.file_name}
                  <button
                    aria-label={`Remove ${a.file_name}`}
                    className="hover:text-danger"
                    onClick={() => removeAttachment(current.id, i)}
                  >
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}

          {(uploaded.data?.length ?? 0) > 0 && (
            <div className="space-y-1">
              <div className="text-xs text-muted">Previously uploaded:</div>
              <div className="flex flex-wrap gap-1">
                {uploaded.data!.map((b64, i) => (
                  <img
                    key={i}
                    alt={`Uploaded screenshot ${i + 1}`}
                    className="h-16 cursor-zoom-in rounded border border-border object-cover"
                    src={`data:image/png;base64,${b64}`}
                    {...lightbox.getTriggerProps(i)}
                  />
                ))}
              </div>
              <AstryxIsland>{lightbox.element}</AstryxIsland>
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
                {outcomeLabel(o)}
              </button>
            ))}
          </div>
        </div>
      )}

      <footer className="flex items-center gap-2 border-t border-border bg-surface px-3 py-2">
        <Button variant="ghost" size="sm" disabled={idx === 0} onClick={() => setIdx((i) => i - 1)}>
          <IconBack aria-hidden />
          Prev
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={idx >= list.length - 1}
          onClick={() => setIdx((i) => i + 1)}
        >
          <IconNext aria-hidden />
          Next
        </Button>
        <Button
          className="ml-auto"
          size="sm"
          // Latched on success. Every one of the non-closing paths above
          // leaves the window open with the marks still in state and the
          // button re-armed, so a second click created a SECOND Azure
          // DevOps run with all the same outcomes recorded twice - and
          // this app cannot delete a run. The window stays open so the
          // tester can read what failed; it just cannot be sent again.
          disabled={markedCount === 0 || finish.isPending || finish.isSuccess}
          onClick={() => finish.mutate()}
        >
          <IconFinish aria-hidden />
          {finish.isPending
            ? "Recording"
            : finish.isSuccess
              ? "Recorded"
              : `Finish (${markedCount})`}
        </Button>
      </footer>

      {bugFor && (
        <BugDialog
          org={session.org}
          project={session.project}
          testCase={bugFor}
          pbiId={session.pbi.id}
          screenshots={(states[bugFor.id]?.attachments ?? [])
            .filter((a) => IMAGE_EXT.test(a.file_name))
            .map((a) => a.b64)}
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
