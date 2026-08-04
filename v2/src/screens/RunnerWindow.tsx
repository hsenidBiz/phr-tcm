import { useLightbox } from "@astryxdesign/core/Lightbox";
import { useMutation, useQuery } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { Pin, PinOff, X } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
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
import { emitPointRecorded } from "../lib/runnerBus";
import { loadRunnerPinned, loadRunnerSession, saveRunnerPinned } from "../lib/runnerSession";
import { OFFLINE_HINT, onlineSnapshot, subscribeOnline } from "../lib/network";
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

// The same verdicts Azure DevOps's own runner offers: Pass, Fail, Pause,
// Block, Not applicable - "Paused" is a real TestOutcome the API records,
// for a case someone had to stop half way through and means to resume.
const OUTCOMES = ["Passed", "Failed", "Paused", "Blocked", "NotApplicable"] as const;
type Outcome = (typeof OUTCOMES)[number] | "";

type CaseState = {
  outcome: Outcome;
  comment: string;
  stepOutcomes: Record<number, Outcome>;
  attachments: RunAttachment[];
  elapsedMs: number;
  bugIds: number[];
  /** The tester clicked a lit verdict to clear it. The last-outcome
   * pre-select must not re-apply on the next points refetch (refetches
   * happen on every window refocus) - an explicit clear is a decision,
   * not a blank slate. */
  cleared?: boolean;
};

function emptyState(): CaseState {
  return { outcome: "", comment: "", stepOutcomes: {}, attachments: [], elapsedMs: 0, bugIds: [] };
}

const outcomeBtn: Record<string, string> = {
  Passed: "bg-success text-on-accent",
  Failed: "bg-danger text-on-accent",
  // Neutral-dark, not a fourth traffic-light colour: paused is "no verdict
  // yet", and it must not read as a sibling of pass/fail at a glance.
  Paused: "bg-muted text-on-accent",
  Blocked: "bg-warning text-on-accent",
  NotApplicable: "bg-surface-2 text-muted",
};

const outcomeBadge: Record<string, string> = {
  passed: "bg-success/15 text-success",
  failed: "bg-danger/15 text-danger",
  paused: "bg-muted/15 text-muted",
  blocked: "bg-warning/15 text-warning",
  notapplicable: "bg-surface-2 text-muted",
};

// Pre-selection identity for the verdict segments: a small dot says which
// colour each verdict turns without painting five loud buttons. (The dot
// hides on the selected segment - its background carries the colour then.)
const outcomeDot: Record<string, string> = {
  Passed: "bg-success",
  Failed: "bg-danger",
  Paused: "bg-muted",
  Blocked: "bg-warning",
  NotApplicable: "bg-faint",
};

const IMAGE_EXT = /\.(png|jpe?g|gif|bmp|webp)$/i;

/** Data-URL mime from the attachment's extension (snips/pastes are png;
 * Attach file can bring the rest). */
function mimeFor(name: string): string {
  if (/\.jpe?g$/i.test(name)) return "image/jpeg";
  if (/\.gif$/i.test(name)) return "image/gif";
  if (/\.bmp$/i.test(name)) return "image/bmp";
  if (/\.webp$/i.test(name)) return "image/webp";
  return "image/png";
}

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
  const online = useSyncExternalStore(subscribeOnline, onlineSnapshot);

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

  // This session's own image attachments get the same fullscreen viewer:
  // until now they were filename-only chips, so the only way to check WHAT
  // was snipped was to remove it and snip again. Non-image files (Attach
  // file accepts anything; recordings are video) keep the plain chip.
  const sessionImages = st.attachments
    .map((a, index) => ({ ...a, index }))
    .filter((a) => IMAGE_EXT.test(a.file_name));
  const sessionLightbox = useLightbox({
    media: sessionImages.map((a) => ({
      src: `data:${mimeFor(a.file_name)};base64,${a.b64}`,
      alt: a.file_name,
    })),
  });

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
        if (next[p.test_case_id]?.cleared) continue; // tester explicitly un-marked it
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

  /** Stop waiting for a snip (overlay dismissed with Esc, or a change of
   * mind): bumping the token ends the poll loop's current run, and its
   * final-toast guard stays quiet because the token no longer matches. */
  function cancelSnip() {
    snipToken.current++;
    setSnipping(false);
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

  // ---- Incremental recording: Next writes the case being left. -------
  //
  // The run opens LAZILY on the first recorded outcome and stays open (In
  // Progress in Azure DevOps - exactly what ADO's own runner does with a
  // paused session) until Finish completes it. Every recorded outcome is
  // already saved the moment it was recorded, so closing the window loses
  // nothing that was marked and advanced past.
  //
  // A record that fails stays unsynced and retries on the next navigation
  // and again at Finish - recording must never trap the tester on a case.
  // Records are chained one-at-a-time so two Nexts cannot interleave the
  // run creation or land out of order.
  const runRef = useRef<{ runId: number; byPoint: Map<number, number> } | null>(null);
  const startingRef = useRef<Promise<{ runId: number; byPoint: Map<number, number> }> | null>(null);
  const syncedRef = useRef<Record<number, string>>({});
  const recordFailures = useRef<Map<number, string>>(new Map());
  const droppedRef = useRef<Set<number>>(new Set());
  const chainRef = useRef<Promise<void>>(Promise.resolve());

  const ensureRun = () => {
    if (runRef.current) return Promise.resolve(runRef.current);
    if (!startingRef.current) {
      startingRef.current = (async () => {
        // Reuse the points the window already loaded; fetch only if the
        // first Next lands before that query resolves.
        const pts =
          points.data ??
          (await unwrap(
            commands.listTestPoints(session!.org, session!.project, session!.planId, session!.suiteId),
          ));
        const sessionIds = new Set(list.map((c) => c.id));
        const pointIds = pts
          .filter((p) => p.test_case_id != null && sessionIds.has(p.test_case_id))
          .map((p) => p.point_id);
        if (pointIds.length === 0) throw new Error("None of these cases have a test point.");
        const started = await unwrap(
          commands.startTestRun(
            session!.org,
            session!.project,
            session!.planId,
            `${session!.pbi.title} - manual run`,
            pointIds,
          ),
        );
        const run = {
          runId: started.run_id,
          byPoint: new Map(started.results.map((r) => [r.point_id, r.result_id])),
        };
        runRef.current = run;
        return run;
      })().catch((e) => {
        // A failed start must not poison every later attempt - clear the
        // single-flight slot so the next Next tries again.
        startingRef.current = null;
        throw e;
      });
    }
    return startingRef.current;
  };

  /** Leaving a case whose lit verdict was clicked OFF: push ADO's own
   * "reset test" so the point reads Active again - same deferred, chained,
   * retry-on-Next model as recording. Skipped when there is nothing to
   * reset (never recorded this session and never run before either). */
  const queueReset = (c: TestCaseFull) => {
    const point = (points.data ?? []).find((p) => p.test_case_id === c.id);
    if (!point) return;
    const synced = syncedRef.current[c.id];
    if (!synced && !point.last_outcome) return; // already a blank slate
    if (synced === "RESET") return; // this clear is already recorded
    chainRef.current = chainRef.current.then(async () => {
      try {
        await unwrap(
          commands.resetTestPoints(
            session!.org,
            session!.project,
            session!.planId,
            session!.suiteId,
            [point.point_id],
          ),
        );
        syncedRef.current[c.id] = "RESET";
        recordFailures.current.delete(c.id);
        emitPointRecorded({
          org: session!.org,
          project: session!.project,
          planId: session!.planId,
          suiteId: session!.suiteId,
          testCaseId: c.id,
          outcome: "",
          runId: null,
          resultId: null,
        });
      } catch (e) {
        recordFailures.current.set(c.id, (e as Error).message);
        toast.error(
          `${c.title}: could not reset to Active (${(e as Error).message}). It will retry on the next Next or on Finish.`,
          { duration: 8000 },
        );
      }
    });
  };

  /** Record one case's current marks, if they changed since last recorded.
   * Queued behind any record already in flight. */
  const syncCase = (c: TestCaseFull) => {
    const s = states[c.id];
    if (!s?.outcome) {
      if (s?.cleared) queueReset(c);
      return;
    }
    const marked = Object.keys(s.stepOutcomes);
    const payload = {
      point_id: 0, // filled below once the point resolves
      outcome: s.outcome as string,
      comment: s.comment || null,
      duration_ms: s.elapsedMs || null,
      step_ids: marked.length ? c.step_ids : null,
      step_outcomes: marked.length ? c.steps.map((_, i) => s.stepOutcomes[i] || null) : null,
      attachments: s.attachments.length ? s.attachments : null,
      bug_ids: s.bugIds.length ? s.bugIds : null,
    };
    const snap = JSON.stringify(payload);
    if (syncedRef.current[c.id] === snap) return; // already recorded as-is
    // Offline: DEFER, silently. The mark is kept, the tester keeps
    // moving, and the reconnect listener below flushes everything unsent
    // - a record attempted now is known-doomed and would only add a
    // failure toast per case to a connection problem the banner already
    // explains.
    if (!onlineSnapshot()) return;
    chainRef.current = chainRef.current.then(async () => {
      try {
        const run = await ensureRun();
        const point = (points.data ?? []).find((p) => p.test_case_id === c.id);
        const resultId = point && run.byPoint.get(point.point_id);
        if (!point || resultId == null) {
          // Not in the suite, or ADO made no result row: this run can
          // never hold a mark for it. Remembered for the Finish summary,
          // said once rather than on every pass.
          if (!droppedRef.current.has(c.id)) {
            droppedRef.current.add(c.id);
            toast.warning(
              `${c.title} is not in this run - it has no test point, so its mark cannot be recorded.`,
              { duration: 10000 },
            );
          }
          return;
        }
        const extras = await unwrap(
          commands.recordResult(session!.org, session!.project, run.runId, resultId, {
            ...payload,
            point_id: point.point_id,
          }),
        );
        syncedRef.current[c.id] = snap;
        recordFailures.current.delete(c.id);
        // Tell the main window so the Run Tests table repaints this row now.
        emitPointRecorded({
          org: session!.org,
          project: session!.project,
          planId: session!.planId,
          suiteId: session!.suiteId,
          testCaseId: c.id,
          outcome: s.outcome as string,
          runId: run.runId,
          resultId,
        });
        // Additive extras that did not stick - same register as the batch
        // flow: recorded, but this did not attach.
        for (const x of extras) {
          toast.warning(`Recorded, but this did not attach: ${x}. Add it in Azure DevOps.`, {
            duration: 10000,
          });
        }
      } catch (e) {
        recordFailures.current.set(c.id, (e as Error).message);
        toast.error(
          `${c.title}: not recorded yet (${(e as Error).message}). It will retry on the next Next or on Finish.`,
          { duration: 8000 },
        );
      }
    });
  };

  // The connection coming back flushes every marked-but-unsent case.
  useEffect(() => {
    return subscribeOnline(() => {
      if (!onlineSnapshot()) return;
      for (const c of list) syncCase(c);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [list, states]);

  /** Navigation is also the save: the case being LEFT is recorded. */
  const goTo = (next: number) => {
    if (current) syncCase(current);
    setIdx(next);
  };

  const finish = useMutation({
    mutationFn: async () => {
      // Flush everything marked - the current case, and any earlier
      // failure that has not been retried since.
      for (const c of list) syncCase(c);
      await chainRef.current;

      const recorded = Object.keys(syncedRef.current).length;
      if (recorded === 0 || !runRef.current) {
        const stuck = recordFailures.current.size;
        throw new Error(
          stuck > 0
            ? "Nothing could be recorded - check the connection and press Finish again."
            : "Mark at least one case first.",
        );
      }
      const failures = [...recordFailures.current.entries()];
      await unwrap(
        commands.finishTestRun(session!.org, session!.project, runRef.current.runId),
      );
      return { recorded, failures, dropped: droppedRef.current.size };
    },
    onSuccess: ({ recorded, failures, dropped }) => {
      if (failures.length > 0) {
        // The run is complete; these marks are NOT in it. Saying which is
        // the only way to act on it.
        toast.warning(
          `${recorded} result(s) recorded, but ${failures.length} could not be: ` +
            failures.map(([id, e]) => `#${id} (${e})`).join(", ") +
            ". The run is completed without them.",
          { duration: 20000 },
        );
        return; // stay open so the tester can read what happened
      }
      if (dropped > 0) {
        toast.warning(
          `${recorded} result(s) recorded. ${dropped} case(s) had no test point and are not in the run.`,
          { duration: 15000 },
        );
        return;
      }
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
                        onClick={() => {
                          // Same toggle as the overall verdict: clicking the
                          // lit mark clears it (a cleared step just is not
                          // recorded, like one never marked).
                          const next = { ...st.stepOutcomes };
                          if (next[i] === o) delete next[i];
                          else next[i] = o;
                          patch(current.id, { stepOutcomes: next });
                        }}
                      >
                        {o[0]}
                      </button>
                    ))}
                  </div>
                </div>
              </li>
            ))}
          </ol>

          {/* Evidence card: the comment, the capture tools, and the
              attachments they produce are one activity - documenting what
              happened - so they live in one labelled card instead of three
              loose rows floating between the steps and the verdict. shrink-0
              keeps the card from being crushed on short screens. */}
          <div className="shrink-0 space-y-2 rounded-md border border-border p-2">
          <div className="flex items-baseline justify-between">
            <span className="text-xs font-semibold text-muted">Evidence · optional</span>
            {st.attachments.length > 0 && (
              <span className="text-[11px] text-faint">
                {st.attachments.length} attached
              </span>
            )}
          </div>
          {/* min-h + resize-y survive from the short-screen fix: the box can
              never collapse, and grows taller while the width stays fixed. */}
          <Textarea
            aria-label="Comment"
            className="h-16 min-h-16 w-full shrink-0 resize-y text-sm"
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
            <Button
              variant="outline"
              size="sm"
              title={snipping ? "Stop waiting for the snip" : undefined}
              onClick={snipping ? cancelSnip : snip}
            >
              <IconSnip aria-hidden />{snipping ? "Cancel snip" : "Snip"}
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
            <div className="space-y-1">
              {sessionImages.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {sessionImages.map((a, si) => (
                    <span key={a.index} className="relative">
                      <img
                        alt={a.file_name}
                        title={`${a.file_name} - click to view`}
                        className="h-16 cursor-zoom-in rounded border border-border object-cover"
                        src={`data:${mimeFor(a.file_name)};base64,${a.b64}`}
                        {...sessionLightbox.getTriggerProps(si)}
                      />
                      <button
                        aria-label={`Remove ${a.file_name}`}
                        className="absolute -right-1.5 -top-1.5 rounded-full border border-border bg-surface p-0.5 text-muted hover:text-danger"
                        onClick={() => removeAttachment(current.id, a.index)}
                      >
                        <X size={10} />
                      </button>
                    </span>
                  ))}
                  <AstryxIsland>{sessionLightbox.element}</AstryxIsland>
                </div>
              )}
              {st.attachments.some((a) => !IMAGE_EXT.test(a.file_name)) && (
                <div className="flex flex-wrap gap-1">
                  {st.attachments.map((a, i) =>
                    IMAGE_EXT.test(a.file_name) ? null : (
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
                    ),
                  )}
                </div>
              )}
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
          </div>
        </div>
      )}

      <footer className="shrink-0 space-y-2 border-t border-border bg-surface px-3 py-2">
        {/* The verdict is this screen's primary action, so it is pinned here
            under the scroll area - a long case can never hide it. Passed and
            Failed get double width (they are nearly all the clicks); the
            other three stay reachable but read as secondary. */}
        {current && (
          <div className="flex gap-1">
            {OUTCOMES.map((o) => (
              <button
                key={o}
                className={cn(
                  "flex items-center justify-center gap-1.5 rounded-md py-2 text-xs font-semibold",
                  o === "Passed" || o === "Failed" ? "flex-[2]" : "flex-1",
                  st.outcome === o ? outcomeBtn[o] : "border border-border text-muted hover:text-text",
                )}
                onClick={() =>
                  // Click again to un-mark: an unmarked case is simply not
                  // recorded at Finish (elapsed only updates on selection).
                  patch(
                    current.id,
                    st.outcome === o
                      ? { outcome: "", cleared: true }
                      : { outcome: o, cleared: false, elapsedMs: Date.now() - startRef.current },
                  )
                }
              >
                {st.outcome !== o && (
                  <span aria-hidden className={cn("size-1.5 rounded-full", outcomeDot[o])} />
                )}
                {outcomeLabel(o)}
              </button>
            ))}
          </div>
        )}
        <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" disabled={idx === 0} onClick={() => goTo(idx - 1)}>
          <IconBack aria-hidden />
          Prev
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={idx >= list.length - 1}
          onClick={() => goTo(idx + 1)}
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
          // Also gated offline: Finish COMPLETES the run in Azure DevOps,
          // which cannot happen on a dead connection - the marks are all
          // kept, and the reconnect flush sends anything unsent first.
          disabled={markedCount === 0 || finish.isPending || finish.isSuccess || !online}
          title={online ? undefined : OFFLINE_HINT}
          onClick={() => finish.mutate()}
        >
          <IconFinish aria-hidden />
          {finish.isPending
            ? "Recording"
            : finish.isSuccess
              ? "Recorded"
              : `Finish (${markedCount})`}
        </Button>
        </div>
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
