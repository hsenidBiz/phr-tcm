// Driving one case while a person watches.
//
// The action outcomes are EVIDENCE, never a vote. Nothing here
// pre-selects a verdict: a green run can still be a failure the person
// spotted with their eyes, and a red action can be the harness's fault
// rather than the app's. The human presses the button.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, events, type ActionOutcome, type CaseRecord, type ReplayEnd, type SignInOutcome } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Textarea } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { dbReadAccessOn } from "../../lib/mcpTools";
import VerdictPicker from "./VerdictPicker";

const BROWSERS = [
  { value: "edge", label: "Microsoft Edge" },
  { value: "chrome", label: "Google Chrome" },
];

/** How the app's refusal begins when a no-save case's guard cannot start. */
const GUARD_NOT_SET_UP = "the no-save guard could not be set up: ";

/** The outcome a step the replay ran is saved with. */
const REPLAYED_OUTCOME = "replayed before healing";

/** How a replay's answer is coloured, by how it ended. */
const REPLAY_TONE: Record<ReplayEnd["kind"], string> = {
  ready: "text-success",
  stopped_at: "text-danger",
  stopped: "text-muted",
  refused: "text-warning",
};

export default function RunPane({
  org,
  project,
  pbiId,
  cases,
  replayTo,
  onClose,
}: {
  org: string;
  project: string;
  pbiId: number;
  /** The selection, run one after another in this order. One case is a
   * selection of one - there is no separate single-case path. */
  cases: { id: number; title: string }[];
  /** Set when the pane opens for Replay to step N (Past runs, the review):
   * the first case's saved steps before this one are replayed at once, in a
   * browser the replay opens itself when none is open, and the person
   * carries on from this step by hand. */
  replayTo?: number;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();

  // Where we are in the selection.
  const [idx, setIdx] = useState(0);
  const current = cases[idx] ?? cases[0];
  const caseId = current?.id ?? 0;
  const title = current?.title ?? "";
  const isLast = idx >= cases.length - 1;

  /** Verdicts marked so far. Kept here rather than written per case so a
   * bulk run is ONE run in the results view, the way a person thinks of
   * it - and flushed on close as well as on finish, so walking away
   * half-way through does not throw away the verdicts already given. */
  const [records, setRecords] = useState<CaseRecord[]>([]);

  /** Which browser to watch in. Remembered, because a person who prefers
   * Chrome prefers it every time - but only a name the picker can show:
   * anything else would leave the dropdown blank while Rust quietly fell
   * back to Edge, so the screen and the run would disagree. */
  const [browserName, setBrowserName] = useState(() => {
    const saved = localStorage.getItem("tcm-v2-autorun-browser");
    return BROWSERS.some((b) => b.value === saved) ? (saved as string) : "edge";
  });

  /** When the person started, not when the file happened to be written -
   * a twelve-case selection takes long enough that stamping it at the end
   * would sort the run under the wrong time. */
  const [startedAt] = useState(() => String(Date.now()));

  const script = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrapStr(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const [opened, setOpened] = useState(false);
  const [busy, setBusy] = useState(false);
  /** Save and Close both write and both end the session, so exactly one
   * of them may be in flight. Without this, a backdrop click landing
   * while Save awaits its write starts a SECOND write from a shorter
   * record list - and `new_run_id` is millisecond-resolution, so the two
   * can collide on one filename and the shorter list wins. */
  const inFlight = useRef(false);
  const [saving, setSaving] = useState(false);
  const [results, setResults] = useState<Record<number, ActionOutcome[]>>({});
  const [verdict, setVerdict] = useState("");
  const verdictLabelId = useId();
  const [note, setNote] = useState("");

  /** How many browsers this pane has successfully opened. A sign-in belongs
   * to a BROWSER, not to a case: keying it on the case would fire it at the
   * old window in the moment between moving on and the fresh one opening. */
  const [launches, setLaunches] = useState(0);
  const signedFor = useRef(0);
  // Mirrors `launches` for `signInAs` to read AFTER its await, when the
  // closure's own `launches` is frozen at whatever it was when that call
  // started - see the comment in `signInAs`.
  const launchRef = useRef(0);
  useEffect(() => {
    launchRef.current = launches;
  }, [launches]);
  const [signIn, setSignIn] = useState<{ state: "idle" | "working" | "done"; account: string; out: SignInOutcome | null }>({
    state: "idle",
    account: "",
    out: null,
  });
  /** The current case's preconditions, checked once per browser before its
   * sign-in. "blocked" carries the sentence: the case never signs in and
   * none of its steps can run. `notice` is said and the case goes on (the
   * checks were skipped while Database Read Access is off). */
  const [pre, setPre] = useState<{ state: "idle" | "checking" | "blocked"; reason: string; notice: string }>({
    state: "idle",
    reason: "",
    notice: "",
  });

  // The failure screenshot on show, as a data URL, or null.
  const [shot, setShot] = useState<string | null>(null);
  const openShot = (name: string) =>
    unwrapStr(commands.autoRunShot(name))
      .then(setShot)
      .catch((e) => toast.error(`Could not open the screenshot: ${e.message ?? e}`));

  // The browser is a real Edge/Chrome process with a temp profile directory - it
  // has to be closed on every path that ends this session, not just the
  // ones the brief spells out (Close, Save). If the person navigates away
  // instead, this component unmounts without either firing, and the
  // process + its temp dir would otherwise leak for the rest of the app's
  // lifetime. `openedRef` (not state) means this effect's cleanup always
  // sees the latest answer without re-subscribing on every render, and the
  // `closedRef` guard stops it from ever firing twice (unmount racing an
  // in-flight explicit close).
  const openedRef = useRef(false);
  const closedRef = useRef(false);
  useEffect(() => {
    openedRef.current = opened;
  }, [opened]);
  useEffect(() => {
    return () => {
      if (openedRef.current && !closedRef.current) {
        closedRef.current = true;
        void commands.autoRunCloseBrowser().catch(() => {});
      }
    };
  }, []);

  const openBrowser = async () => {
    setBusy(true);
    try {
      const r = await commands.autoRunOpenBrowser(browserName);
      if (r.status === "error") {
        toast.error(`Could not open the browser: ${r.error}`);
        return;
      }
      setOpened(true);
      setLaunches((n) => n + 1);
    } catch (e) {
      // The generated `typedError` wrapper rethrows when the IPC call itself
      // rejects with an Error (rather than resolving to {status: "error"}) -
      // without this catch that propagates as an unhandled rejection AND,
      // because setBusy(false) below never runs, leaves this button
      // permanently disabled with no explanation.
      toast.error(`Could not open the browser: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const runStep = async (stepNumber: number) => {
    const step = script.data?.steps.find((s) => s.step_number === stepNumber);
    if (!step) return;
    setBusy(true);
    try {
      const r = await commands.autoRunStep(org, project, caseId, step);
      if (r.status === "error") {
        // A no-save case whose guard could not be set up never runs
        // unguarded: Blocked, with the sentence, as an unattended run
        // records it - the same way a precondition blocks a case.
        if (r.error.startsWith(GUARD_NOT_SET_UP)) {
          setPre((p) => ({ state: "blocked", reason: r.error, notice: p.notice }));
          return;
        }
        toast.error(r.error);
        return;
      }
      setResults((prev) => ({ ...prev, [stepNumber]: r.data }));
    } catch (e) {
      // See openBrowser above: a rethrown Error here would otherwise wedge
      // every "Run step N" button disabled for the rest of the session.
      toast.error(`Could not run step ${stepNumber}: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const signInAs = async (account: string, afresh: boolean) => {
    // The Rust side takes one SESSION mutex per browser command, so this
    // result cannot land ON TOP of a browser close - it just queues behind
    // one. What it CAN do is land AFTER the pane has already moved on to
    // the next case's fresh browser (a new `launches`): `forLaunch` is
    // this render's value, frozen for the life of this call; `launchRef`
    // is mutable, so comparing them after the await tells whether this
    // result is still for the browser the person is looking at.
    const forLaunch = launches;
    setBusy(true);
    setSignIn({ state: "working", account, out: null });
    try {
      if (afresh) await commands.autoRunForgetSession(account).catch(() => {});
      const r = await commands.autoRunSignIn(org, project, account);
      const out: SignInOutcome =
        r.status === "error" ? { ok: false, detail: r.error, used_saved_session: false, steps: [] } : r.data;
      if (launchRef.current === forLaunch) setSignIn({ state: "done", account, out });
    } catch (e) {
      const detail = e instanceof Error ? e.message : String(e);
      if (launchRef.current === forLaunch) {
        setSignIn({ state: "done", account, out: { ok: false, detail, used_saved_session: false, steps: [] } });
      }
    } finally {
      // A result for a launch the pane has already moved past must not
      // clear busy: a newer launch's own sign-in (or step) may genuinely
      // be running right now, and this stale call finishing must not
      // re-enable its buttons out from under it.
      if (launchRef.current === forLaunch) setBusy(false);
    }
  };

  // Once per browser, and only once the CURRENT case's script is in hand:
  // while the next case's script is still loading, `script.data` is
  // undefined (a new query key has no data yet), so this waits for it.
  // Also waits out `busy`: without it, this rests on effect timing alone
  // to never overlap a browser command already in flight (a step, another
  // sign-in) - an explicit guard instead of a lucky race.
  const scriptAccount = script.isSuccess ? (script.data?.account ?? "") : null;
  const hasPreconditions = (script.data?.preconditions?.length ?? 0) > 0;
  useEffect(() => {
    if (!opened || launches === 0 || scriptAccount === null || busy) return;
    if (signedFor.current === launches) return;
    signedFor.current = launches;
    if (hasPreconditions) {
      void startCase(scriptAccount);
      return;
    }
    if (scriptAccount === "") {
      setSignIn({ state: "idle", account: "", out: null });
      return;
    }
    void signInAs(scriptAccount, false);
    // startCase and signInAs are recreated every render; the values below
    // are the only things that should start a case.
  }, [opened, launches, scriptAccount, hasPreconditions, busy]);

  /** A case whose script has preconditions: the app checks them first, and
   * signs the case in only when every one is met. A case Blocked here keeps
   * the sentence as its reason, never signs in, and runs no step. */
  const startCase = async (account: string) => {
    const forLaunch = launches;
    setBusy(true);
    setPre({ state: "checking", reason: "", notice: "" });
    let blocked: string | null;
    let notice = "";
    try {
      const r = await commands.autoRunCheckPreconditions(org, project, caseId, dbReadAccessOn());
      if (r.status === "error") {
        blocked = `precondition could not be checked: ${r.error}`;
      } else {
        blocked = r.data.blocked;
        notice = r.data.notice ?? "";
      }
    } catch (e) {
      blocked = `precondition could not be checked: ${e instanceof Error ? e.message : String(e)}`;
    }
    // See signInAs: a result for a browser the pane has moved past is
    // dropped, and must not clear a newer launch's busy flag.
    if (launchRef.current !== forLaunch) return;
    if (blocked) {
      setPre({ state: "blocked", reason: blocked, notice: "" });
      setBusy(false);
      return;
    }
    setPre({ state: "idle", reason: "", notice });
    if (account === "") {
      setBusy(false);
      return;
    }
    await signInAs(account, false);
  };

  /** A replay to a step: going (with the step it is on, once the first
   * progress arrives), or ended with the sentence the app answered, said as
   * it is. */
  const [replay, setReplay] = useState<
    | { state: "going"; at: { step: number; of: number } | null }
    | { state: "ended"; kind: ReplayEnd["kind"]; sentence: string }
    | null
  >(null);
  /** The step a replay stopped before: every step below it ran in the
   * replay. 0 while nothing was replayed. */
  const [replayedTo, setReplayedTo] = useState(0);

  /** Replay this case's steps before `step` in the supervised browser, and
   * leave the pane on `step` with that browser as its own: the replay signed
   * the case in, so the pane does not sign in over it. */
  const startReplay = async (step: number) => {
    const wasOpen = opened;
    const forCase = caseId;
    setBusy(true);
    setReplay({ state: "going", at: null });
    // Listening before the replay starts, so its first step is never missed.
    // A listener that could not be set up costs the progress line, never the
    // replay.
    const unlisten = await events.autorunReplayProgress
      .listen((e) => {
        const p = e.payload;
        if (p.case_id !== forCase) return;
        setReplay((r) => (r?.state === "going" ? { state: "going", at: { step: p.step, of: p.of } } : r));
      })
      .catch(() => null);
    try {
      const r = await commands.autoRunReplayToStep(org, project, caseId, step, dbReadAccessOn());
      if (r.status === "error") {
        setReplay(null);
        toast.error(`Could not open the browser: ${r.error}`);
        return;
      }
      const { end, sentence } = r.data;
      setReplay({ state: "ended", kind: end.kind, sentence });
      // A refusal may come before or after the browser opened: the pane
      // keeps what it showed, and closes whatever is there when it goes.
      openedRef.current = true;
      if (end.kind === "refused") return;
      if (!wasOpen) {
        const launch = launchRef.current + 1;
        signedFor.current = launch;
        launchRef.current = launch;
        setLaunches(launch);
        setOpened(true);
      }
      if (end.kind === "ready") {
        setReplayedTo(end.detail.step);
        const notice = end.detail.notice;
        if (notice) setPre((p) => ({ ...p, notice }));
      } else if (end.kind === "stopped_at") {
        const { step: k, outcomes } = end.detail;
        setReplayedTo(k);
        if (outcomes.length > 0) setResults((prev) => ({ ...prev, [k]: outcomes }));
      } else {
        setReplayedTo(end.detail.step);
      }
    } catch (e) {
      setReplay(null);
      toast.error(`Could not open the browser: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      unlisten?.();
      setBusy(false);
    }
  };

  // A pane opened for a replay starts it once, at once.
  const replayStarted = useRef(false);
  useEffect(() => {
    if (replayTo === undefined || replayStarted.current) return;
    replayStarted.current = true;
    void startReplay(replayTo);
    // Once per pane: startReplay is recreated every render.
  }, [replayTo]);

  /** The verdict in front of the person right now, as a record. */
  const currentRecord = (): CaseRecord => ({
    case_id: caseId,
    title,
    verdict,
    note,
    steps: (script.data?.steps ?? []).map((s) => ({
      step_number: s.step_number,
      // A step the replay ran has no outcomes of its own to keep: it is
      // saved as replayed, never as an empty step.
      outcomes:
        results[s.step_number] ??
        (s.step_number < replayedTo ? [{ ok: true, detail: REPLAYED_OUTCOME }] : []),
    })),
    // A case its preconditions blocked says so the way an unattended run
    // does: proposed Blocked, with the sentence. The verdict stays theirs.
    ...(pre.state === "blocked" ? { proposed: "Blocked", reason: pre.reason } : {}),
    // Checks skipped while Database Read Access was off: said on the record
    // too, as an unattended run says it.
    ...(pre.notice ? { notice: pre.notice } : {}),
  });

  /** Every case starts from a clean browser. Keeping one profile across
   * the selection would let case 2 pass only because case 1 signed in -
   * a green that vanishes the moment the case is run on its own, which is
   * the one result a test runner must never produce. A relaunch that
   * fails drops back to the explicit button rather than leaving the
   * person driving a window that is no longer there. */
  const freshBrowser = async () => {
    await commands.autoRunCloseBrowser().catch(() => {});
    try {
      const r = await commands.autoRunOpenBrowser(browserName);
      if (r.status === "error") throw new Error(r.error);
      setLaunches((n) => n + 1);
    } catch (e) {
      setOpened(false);
      toast.error(
        `Could not open a fresh browser for the next case: ${
          e instanceof Error ? e.message : String(e)
        }`,
      );
    }
  };

  const save = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true);
    try {
      const record = currentRecord();

      // More cases to go: bank this verdict and move on WITHOUT writing.
      // One selection is one run, so nothing reaches disk until the last
      // verdict is in (or the person walks away).
      if (!isLast) {
        setRecords((r) => [...r, record]);
        setIdx((i) => i + 1);
        setResults({});
        setVerdict("");
        setNote("");
        setSignIn({ state: "idle", account: "", out: null });
        setPre({ state: "idle", reason: "", notice: "" });
        await freshBrowser();
        return;
      }

      if (!(await writeRun([...records, record]))) return;
      closedRef.current = true;
      await commands.autoRunCloseBrowser().catch(() => {});
      onClose();
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  };

  /** The one place a run reaches disk. Returns false when it did not, so
   * callers can leave the pane open rather than closing over a failure. */
  const writeRun = async (all: CaseRecord[]): Promise<boolean> => {
    if (all.length === 0) return true;
    let id: string;
    try {
      id = await commands.autoRunNewId();
    } catch (e) {
      toast.error(`Could not save the result: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
    try {
      const r = await commands.autoRunSaveRun({
        id,
        pbi_id: pbiId,
        started_at: startedAt,
        cases: all,
      });
      if (r.status === "error") {
        toast.error(`Could not save the result: ${r.error}`);
        return false;
      }
    } catch (e) {
      // Same rethrow hazard as openBrowser/runStep: an Error out of the IPC
      // call would otherwise reject unhandled and leave the person staring
      // at a "Save result" click that appeared to do nothing.
      toast.error(`Could not save the result: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
    toast.success(
      all.length === 1
        ? "Result saved on this machine."
        : `${all.length} results saved on this machine.`,
    );
    // What this run says about the project's quirks, counted once, here,
    // now that it is on disk - never from the review screen, whose saves
    // re-write a run already counted. Bookkeeping only: a failure to count
    // is logged on the Rust side and never stands in the way of the save.
    await commands.autoRunCountEvidence(org, project, id).catch(() => null);
    await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
    // Counting may have cleared a suspected-defect mark on a script; the
    // case rows read each script through their own query.
    await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
    return true;
  };

  const close = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true);
    try {
      // Verdicts already marked are not thrown away by walking off - and
      // that includes the one on the case in front of them, which the
      // buttons show as chosen even though Save was never pressed.
      const pending = verdict ? [...records, currentRecord()] : records;
      // A failed write keeps the pane open, the same as Save: closing over
      // it would drop every banked verdict with no way back.
      if (!(await writeRun(pending))) return;
      closedRef.current = true;
      await commands.autoRunCloseBrowser().catch(() => {});
      onClose();
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  };

  return (
    <Modal onClose={close} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
        {cases.length > 1 && (
          <span className="ml-2 text-xs font-normal text-faint">
            case {idx + 1} of {cases.length}
          </span>
        )}
      </h2>

      {replay?.state === "going" && (
        <div className="flex items-center justify-between gap-2 rounded border border-border/60 px-2 py-1 text-xs">
          <span role="status" className="text-muted">
            {replay.at ? `replaying step ${replay.at.step} of ${replay.at.of}` : "Starting the replay"}
          </span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void commands.autoRunStopReplay().catch(() => {})}
          >
            Stop replay
          </Button>
        </div>
      )}
      {replay?.state === "ended" && (
        <p role="status" className={cn("rounded border border-border/60 px-2 py-1 text-xs", REPLAY_TONE[replay.kind])}>
          {replay.sentence}
        </p>
      )}

      {!opened ? (
        // A replay opens its own browser: nothing to pick while it goes.
        replay?.state === "going" ? null : (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            A real browser window opens with a fresh profile. Keep it beside this one and
            watch each step as it runs.
          </p>
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted">
              Browser
              <Select
                aria-label="Browser to run in"
                className="ml-2 w-40"
                value={browserName}
                onChange={(e) => {
                  setBrowserName(e.target.value);
                  try {
                    localStorage.setItem("tcm-v2-autorun-browser", e.target.value);
                  } catch {
                    // storage unavailable - the choice lasts this session
                  }
                }}
              >
                {BROWSERS.map((b) => (
                  <option key={b.value} value={b.value}>
                    {b.label}
                  </option>
                ))}
              </Select>
            </label>
            <Button size="sm" disabled={busy} onClick={openBrowser}>
              Open browser
            </Button>
          </div>
        </div>
        )
      ) : (
        <div className="space-y-2">
          {pre.state === "checking" && (
            <p className="rounded border border-border/60 px-2 py-1 text-xs text-muted">Checking the preconditions</p>
          )}
          {pre.notice && (
            <p role="status" className="rounded border border-warning/40 px-2 py-1 text-xs text-warning">
              {pre.notice}
            </p>
          )}
          {pre.state === "blocked" && (
            <p role="status" className="rounded border border-border/60 px-2 py-1 text-xs text-danger">
              Blocked before step 1: {pre.reason}
            </p>
          )}
          {signIn.state !== "idle" && (
            <div className="rounded border border-border/60 px-2 py-1 text-xs">
              {signIn.state === "working" ? (
                <span className="text-muted">Signing in as {signIn.account}</span>
              ) : (
                <>
                  <div className="flex items-center justify-between gap-2">
                    <span className={signIn.out?.ok ? "text-success" : "text-danger"}>{signIn.out?.detail}</span>
                    <Button size="sm" variant="ghost" disabled={busy} onClick={() => signInAs(signIn.account, true)}>
                      Sign in again
                    </Button>
                  </div>
                  {!signIn.out?.ok && (signIn.out?.steps.length ?? 0) > 0 && (
                    <ul className="mt-1 space-y-0.5 text-faint">
                      {signIn.out?.steps.map((o, i) => (
                        <li key={i} className={o.ok ? "" : "text-danger"}>
                          {o.detail}
                        </li>
                      ))}
                    </ul>
                  )}
                </>
              )}
            </div>
          )}
          <ul className="max-h-72 space-y-2 overflow-y-auto">
            {(script.data?.steps ?? []).map((s) => {
              // After a replay: the steps below where it stopped ran in it,
              // and the one it stopped before is the person's to run next.
              const replayed = s.step_number < replayedTo;
              const next = replayedTo > 0 && s.step_number === replayedTo;
              return (
                <li
                  key={s.step_number}
                  aria-current={next ? "step" : undefined}
                  className={cn("rounded-md border p-2", next ? "border-accent" : "border-border")}
                >
                  <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-muted">Step {s.step_number}</span>
                    <span className="text-[11px] text-faint">
                      {s.actions.length} action{s.actions.length === 1 ? "" : "s"}
                    </span>
                    {replayed && <span className="text-[11px] text-success">replayed</span>}
                    {next && <span className="text-[11px] text-accent">next</span>}
                    <Button
                      className="ml-auto"
                      size="sm"
                      variant="outline"
                      disabled={busy || pre.state !== "idle"}
                      onClick={() => runStep(s.step_number)}
                    >
                      Run step {s.step_number}
                    </Button>
                  </div>
                  {(results[s.step_number] ?? []).map((o, i) => (
                    <p
                      key={i}
                      className={cn("mt-1 text-xs", o.ok ? "text-muted" : "text-danger")}
                    >
                      {o.detail}
                      {o.screenshot && (
                        <button
                          type="button"
                          aria-label={`View screenshot for action ${i + 1}`}
                          className="ml-2 text-muted underline hover:text-accent"
                          onClick={() => openShot(o.screenshot!)}
                        >
                          View screenshot
                        </button>
                      )}
                    </p>
                  ))}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      <div className="space-y-2 border-t border-border pt-3">
        <span id={verdictLabelId} className="text-xs font-medium text-muted">
          Your verdict
        </span>
        <VerdictPicker value={verdict} onPick={setVerdict} labelledBy={verdictLabelId} />
        <Textarea
          aria-label="Result note"
          className="h-16 w-full text-xs"
          placeholder="What you saw (optional)"
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={saving} onClick={close}>
          <IconCancel aria-hidden />
          Close
        </Button>
        <Button
          size="sm"
          // Not the whole `busy` flag - Close must stay available while a
          // sign-in runs, the mutex makes that safe. Blocked here so a
          // verdict is never banked before its own case finished signing in.
          disabled={!verdict || saving || signIn.state === "working"}
          onClick={save}
        >
          <IconConfirm aria-hidden />
          {isLast ? "Save result" : "Save and next case"}
        </Button>
      </div>

      {shot && (
        // max-h-full, not a vh: the backdrop starts below the title bar, so
        // 90vh of the window no longer fits inside it.
        <Modal onClose={() => setShot(null)} className="max-h-full max-w-5xl overflow-auto p-3">
          <img src={shot} alt="Screenshot of the failed action" className="max-w-full" />
        </Modal>
      )}
    </Modal>
  );
}
