// Driving a whole selection unattended: the app opens its own browser per
// case, works through the script alone, and proposes a verdict. Nobody is
// watching by default - the person can leave and come back once it says
// "case N of M" is done.
//
// Nothing here is a verdict. `proposed` is the machine's best guess at what
// a person would have picked; it becomes a real verdict only once someone
// reviews it, and only a reviewed run can ever reach Azure DevOps.

import { useEffect, useId, useRef, useState } from "react";
import { commands, events, type StepRecord } from "../../bindings";
import SharedStepLabel from "../../components/SharedStepLabel";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import {
  IconCancel,
  IconFollowRun,
  IconHideSteps,
  IconShowSteps,
  IconStepDone,
  IconStop,
  IconUnattended,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { useMediaQuery } from "../../lib/useMediaQuery";

// Same two browsers, same values, as the supervised pane's picker - and the
// same storage key, so a person's choice there is their choice here too.
const BROWSERS = [
  { value: "edge", label: "Microsoft Edge" },
  { value: "chrome", label: "Google Chrome" },
];

/** A window this tall has room to show the case after the running one as
 * well, so it opens too. Shorter, only the running case opens. */
const TALL_WINDOW = "(min-height: 900px)";

/** A case's written step, as the script editor shows it. */
export type WrittenStep = { action: string; expected: string; shared?: number | null };

/** How a finished case's written step went. */
type StepResult = { kind: "passed" } | { kind: "failed"; detail: string } | { kind: "not_reached" };

/** The keys that scroll a list when focus is inside it. */
const SCROLL_KEYS = new Set(["PageUp", "PageDown", "Home", "End", "ArrowUp", "ArrowDown"]);

/** One written step's result from the saved run's records: passed when every
 * action of it succeeded, failed with the first failing action's words, and
 * not reached when the run never recorded the step. */
function resultOf(records: readonly StepRecord[], stepNumber: number): StepResult {
  const rec = records.find((r) => r.step_number === stepNumber);
  if (!rec) return { kind: "not_reached" };
  const bad = rec.outcomes.find((o) => !o.ok);
  return bad ? { kind: "failed", detail: bad.detail } : { kind: "passed" };
}

/** Where the account picked for a run is remembered: per organisation and
 * project, on this machine only. */
export function runAccountKey(org: string, project: string): string {
  return `tcm-v2-autorun-run-account:${org}/${project}`;
}

/** The one line a row shows for the phase an unattended step is in - a pure
 * function so a test can drive it directly instead of through an event. */
export function statusOf(p: {
  phase: string;
  step_number: number;
  steps: number;
  proposed: string;
}): string {
  if (p.phase === "opening") return "Opening the browser";
  if (p.phase === "signing_in") return "Signing in";
  if (p.phase === "module") return "Going to the module";
  if (p.phase === "step") return `Step ${p.step_number} of ${p.steps}`;
  if (p.phase === "done") return p.proposed ? `Proposed: ${p.proposed}` : "Nothing proposed";
  return "Waiting";
}

export default function ReplayPane({
  org,
  project,
  pbiId,
  cases,
  onClose,
  onFinished,
}: {
  org: string;
  project: string;
  pbiId: number;
  /** The selection, run in this order - same contract as the supervised
   * pane's `cases` prop. `module` is the case's Module field. */
  cases: { id: number; title: string; module?: string; steps?: WrittenStep[] }[];
  onClose: () => void;
  /** Called with the finished run's id once `auto_run_replay` resolves.
   * The review screen opens from it. */
  onFinished: (runId: string) => void;
}) {
  const [phase, setPhase] = useState<"setup" | "running" | "failed">("setup");
  const [error, setError] = useState("");

  /** Same key the supervised pane uses - a stored value the picker cannot
   * show falls back to Edge, same reasoning as there. */
  const [browserName, setBrowserName] = useState(() => {
    const saved = localStorage.getItem("tcm-v2-autorun-browser");
    return BROWSERS.some((b) => b.value === saved) ? (saved as string) : "edge";
  });
  /** Off by default: an unattended run's whole point is that nobody has to
   * sit in front of it. */
  const [watch, setWatch] = useState(() => localStorage.getItem("tcm-v2-autorun-watch") === "1");

  /** The tester's accounts, key and name only: the Sign in as choices. */
  const [accounts, setAccounts] = useState<{ key: string; label: string }[]>([]);
  useEffect(() => {
    let live = true;
    commands
      .autoRunListAccounts()
      .then((r) => {
        if (live && r.status === "ok") {
          setAccounts((r.data ?? []).map((a) => ({ key: a.key, label: a.label })));
        }
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);
  const [picked, setPicked] = useState(() => {
    try {
      return localStorage.getItem(runAccountKey(org, project)) ?? "";
    } catch {
      return "";
    }
  });
  /** An account removed since it was picked falls back to the default,
   * silently: the run just uses each script's own account. */
  const runAccount = accounts.some((a) => a.key === picked) ? picked : "";

  /** Status text per case id, filled in as `ReplayProgress` events arrive. */
  const [rows, setRows] = useState<Record<number, string>>({});
  /** The selection's own "case N of M" line - null until the first event. */
  const [position, setPosition] = useState<{ index: number; total: number } | null>(null);
  const [stopping, setStopping] = useState(false);
  /** The last phase seen per case id: "done" marks a finished case. */
  const [phases, setPhases] = useState<Record<number, string>>({});
  /** The newest progress event's case and where it is - the running case is
   * this one unless the event said "done". */
  const [latest, setLatest] = useState<{ caseId: number; phase: string; step: number } | null>(
    null,
  );
  /** A finished case's recorded steps, loaded from the run file after its
   * "done" event. Absent when the load failed: the row keeps to its status. */
  const [records, setRecords] = useState<Record<number, StepRecord[]>>({});
  /** Rows the person opened (true) or closed (false) by hand. Cleared for a
   * case when it starts and again when it finishes, so the automatic
   * behaviour takes over at both. */
  const [byHand, setByHand] = useState<Record<number, boolean>>({});
  const tall = useMediaQuery(TALL_WINDOW);
  const idPrefix = useId();

  const rowRefs = useRef(new Map<number, HTMLLIElement>());
  /** True once the person has scrolled the list themselves: the list stops
   * following the run until Follow the run. A ref beside the state because
   * the scroll effect reads it without re-running on it. */
  const pausedRef = useRef(false);
  const [paused, setPausedState] = useState(false);
  const setPaused = (on: boolean) => {
    pausedRef.current = on;
    setPausedState(on);
  };
  /** A press landed on the list itself (its scrollbar, or the gap between
   * rows) and has not been released: a scroll now is the person's. */
  const pressedOnList = useRef(false);

  /** The run this pane's own Start kicked off. Only set from the FIRST
   * progress event seen after Start - a stray event from a run this pane
   * did not start (a previous one that outlived its pane, say) must not
   * overwrite what is on screen for the run actually in progress. */
  const runId = useRef<string | null>(null);

  useEffect(() => {
    const un = events.replayProgress.listen((e) => {
      const p = e.payload;
      if (runId.current === null) runId.current = p.run_id;
      if (p.run_id !== runId.current) return;
      setRows((prev) => ({ ...prev, [p.case_id]: statusOf(p) }));
      setPhases((prev) => ({ ...prev, [p.case_id]: p.phase }));
      setLatest({ caseId: p.case_id, phase: p.phase, step: p.step_number });
      setPosition({ index: p.index, total: p.total });
      // A case starting is locked open and a finished one folds: either way
      // the person's own choice for it is spent.
      setByHand((prev) => {
        if (!(p.case_id in prev)) return prev;
        const { [p.case_id]: _spent, ...rest } = prev;
        return rest;
      });
      const rid = runId.current;
      if (p.phase === "done" && rid) {
        // The run file is saved after every case. Never blocks the run: a
        // failed load leaves the row on its status line.
        commands
          .autoRunLoadRun(rid)
          .then((r) => {
            if (runId.current !== rid || r.status !== "ok" || !r.data) return;
            const rec = r.data.cases.find((x) => x.case_id === p.case_id);
            if (rec) setRecords((prev) => ({ ...prev, [p.case_id]: rec.steps }));
          })
          .catch(() => {});
      }
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const start = async () => {
    setPhase("running");
    setError("");
    setRows({});
    setPosition(null);
    setStopping(false);
    setPhases({});
    setLatest(null);
    setRecords({});
    setByHand({});
    setPaused(false);
    runId.current = null;
    try {
      const r = await commands.autoRunReplay(
        org,
        project,
        pbiId,
        cases.map((c) => ({ case_id: c.id, title: c.title, module: c.module?.trim() || null })),
        runAccount || null,
        browserName,
        watch,
      );
      if (r.status === "error") {
        setError(r.error);
        setPhase("failed");
        return;
      }
      onFinished(r.data.id);
    } catch (e) {
      // Same rethrow hazard as the supervised pane's own IPC calls - the
      // generated wrapper rethrows an Error rather than resolving to
      // {status: "error"} when the call itself rejects.
      setError(e instanceof Error ? e.message : String(e));
      setPhase("failed");
    }
  };

  const stop = async () => {
    // Said and disabled the instant the person presses it - the run itself
    // only stops after its current step, and there is nothing else useful
    // to tell them until it does.
    setStopping(true);
    await commands.autoRunReplayCancel().catch(() => {});
  };

  /** The case running now: the newest event's, unless that said "done". */
  const runningId = latest && latest.phase !== "done" ? latest.caseId : null;
  /** The case after the running one - or, between two cases, after the one
   * that just finished, which is about to start. */
  const afterId = runningId ?? (latest ? latest.caseId : null);
  const afterAt = afterId === null ? -1 : cases.findIndex((c) => c.id === afterId);
  const nextId = afterAt >= 0 ? (cases[afterAt + 1]?.id ?? null) : null;

  /** The running case is always open; the next opens too in a tall window;
   * anything else is as the person left it. */
  const isOpen = (id: number) => id === runningId || (byHand[id] ?? (tall && id === nextId));

  const toggleRow = (id: number) => setByHand((prev) => ({ ...prev, [id]: !isOpen(id) }));

  const scrollToRow = (id: number | null) => {
    if (id !== null) rowRefs.current.get(id)?.scrollIntoView({ block: "nearest" });
  };

  // Follow the run: bring the case that just started into view. Only a change
  // of case scrolls, and never while the person has the scroll.
  useEffect(() => {
    if (!pausedRef.current) scrollToRow(runningId);
  }, [runningId]);

  // A press on the list ends when the pointer is released anywhere.
  useEffect(() => {
    const release = () => {
      pressedOnList.current = false;
    };
    const ends = ["pointerup", "pointercancel", "mouseup"] as const;
    for (const e of ends) window.addEventListener(e, release);
    return () => {
      for (const e of ends) window.removeEventListener(e, release);
    };
  }, []);

  /** The dialog closes on Escape/backdrop everywhere except while a run is
   * actually going - closing the window would not stop it, only Stop does,
   * so letting it look closeable there would be a lie. */
  const closeIfIdle = () => {
    if (phase === "running") return;
    onClose();
  };

  return (
    <Modal onClose={closeIfIdle} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">Unattended run</h2>

      {phase !== "running" ? (
        <div className="space-y-3">
          {error && <p className="text-xs text-danger">{error}</p>}
          <label className="flex items-center gap-2 text-xs text-muted">
            Sign in as
            <Select
              aria-label="Sign in as"
              className="w-56"
              value={runAccount}
              onChange={(e) => {
                setPicked(e.target.value);
                try {
                  localStorage.setItem(runAccountKey(org, project), e.target.value);
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            >
              <option value="">Each script's own account</option>
              {accounts.map((a) => (
                <option key={a.key} value={a.key}>
                  {a.label ? `${a.label} (${a.key})` : a.key}
                </option>
              ))}
            </Select>
          </label>
          <p className="text-xs text-faint">
            An account picked here signs in every case, over the account a script names.
          </p>
          <label className="flex items-center gap-2 text-xs text-muted">
            Browser
            <Select
              aria-label="Browser to run in"
              className="w-40"
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
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox
              checked={watch}
              ariaLabel="Watch the browser"
              onCheckedChange={(on) => {
                setWatch(on);
                try {
                  localStorage.setItem("tcm-v2-autorun-watch", on ? "1" : "0");
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            />
            Watch the browser
          </label>
          <p className="text-xs text-faint">
            Off: the browser runs in the background and you can keep working. On: a window opens
            for every case.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={onClose}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" onClick={start}>
              <IconUnattended aria-hidden />
              Start
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          {position && (
            <p className="text-xs text-muted">
              case {position.index + 1} of {position.total}
            </p>
          )}
          {/* Only the person's own input pauses following - a bare `scroll`
              event is also what scrollIntoView itself causes. */}
          <ul
            aria-label="Cases in this run"
            className="max-h-[min(55vh,32rem)] space-y-1 overflow-y-auto"
            onWheel={() => setPaused(true)}
            onTouchMove={() => setPaused(true)}
            onKeyDown={(e) => {
              if (SCROLL_KEYS.has(e.key)) setPaused(true);
            }}
            onPointerDown={(e) => {
              if (e.target === e.currentTarget) pressedOnList.current = true;
            }}
            onScroll={() => {
              if (pressedOnList.current) setPaused(true);
            }}
          >
            {cases.map((c) => {
              const open = isOpen(c.id);
              const running = c.id === runningId;
              const bodyId = `${idPrefix}-steps-${c.id}`;
              return (
                <li
                  key={c.id}
                  ref={(el) => {
                    if (el) rowRefs.current.set(c.id, el);
                    else rowRefs.current.delete(c.id);
                  }}
                  className="rounded-md border border-border text-xs"
                >
                  <div className="flex items-center gap-2 px-2 py-1.5">
                    <Button
                      variant="ghost"
                      size="sm"
                      className="px-1 py-1 disabled:pointer-events-auto"
                      aria-label={`${open ? "Hide" : "Show"} steps for #${c.id}`}
                      aria-expanded={open}
                      aria-controls={bodyId}
                      title={running ? "Open while it runs" : undefined}
                      disabled={running}
                      onClick={() => toggleRow(c.id)}
                    >
                      {open ? <IconHideSteps aria-hidden /> : <IconShowSteps aria-hidden />}
                    </Button>
                    <span className="id-mono text-faint">#{c.id}</span>
                    <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                    <span
                      className={cn("text-muted", rows[c.id]?.startsWith("Proposed") && "text-text")}
                    >
                      {rows[c.id] ?? "Waiting"}
                    </span>
                  </div>
                  <div id={bodyId} hidden={!open} className="space-y-1 px-2 pb-2">
                    {open && (
                      <CaseSteps
                        org={org}
                        caseId={c.id}
                        steps={c.steps ?? []}
                        status={rows[c.id]}
                        running={running}
                        phase={running ? latest?.phase : undefined}
                        current={running && latest?.phase === "step" ? latest.step : null}
                        finished={phases[c.id] === "done"}
                        records={records[c.id]}
                      />
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
          <div className="flex items-center justify-between gap-2">
            {paused ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setPaused(false);
                  scrollToRow(runningId ?? nextId);
                }}
              >
                <IconFollowRun aria-hidden />
                Follow the run
              </Button>
            ) : (
              <span />
            )}
            <Button size="sm" variant="outline" disabled={stopping} onClick={stop}>
              <IconStop aria-hidden />
              {stopping ? "Stopping after this step" : "Stop"}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}

/** An opened row: the case's written steps, marked as the run reaches them.
 * While it runs, a line says what it is doing before step 1 and the current
 * step stands out; once finished, each step says how it went. */
function CaseSteps({
  org,
  caseId,
  steps,
  status,
  running,
  phase,
  current,
  finished,
  records,
}: {
  org: string;
  caseId: number;
  steps: WrittenStep[];
  status: string | undefined;
  running: boolean;
  phase: string | undefined;
  current: number | null;
  finished: boolean;
  records: StepRecord[] | undefined;
}) {
  if (steps.length === 0) {
    return <p className="text-faint">No written steps to show.</p>;
  }
  const beforeSteps = running && phase !== undefined && phase !== "step";
  return (
    <>
      {beforeSteps && status && <p className="text-muted">{status}</p>}
      <ol aria-label={`Steps of #${caseId}`} className="space-y-1">
        {steps.map((s, i) => {
          const n = i + 1;
          const now = running && current === n;
          const done = running && current !== null && n < current;
          const result = finished && records ? resultOf(records, n) : null;
          return (
            <li
              key={i}
              aria-current={now ? "step" : undefined}
              className={cn(
                "flex items-start gap-2 rounded px-2 py-1",
                now ? "bg-accent-soft text-accent" : "text-muted",
              )}
            >
              <div className="min-w-0 flex-1">
                <span className={now ? "text-accent" : done ? "text-faint" : "text-text"}>
                  {n}.{" "}
                  {s.shared != null ? <SharedStepLabel id={s.shared} org={org} /> : s.action}
                </span>
                {s.shared == null && s.expected && <div className="text-faint">→ {s.expected}</div>}
                {result?.kind === "failed" && <div className="text-danger">{result.detail}</div>}
              </div>
              {now && <span className="shrink-0 font-medium">Checking now</span>}
              {done && (
                <span className="shrink-0 text-success">
                  <IconStepDone aria-hidden />
                  <span className="sr-only">Done</span>
                </span>
              )}
              {result?.kind === "passed" && <span className="shrink-0 text-success">Passed</span>}
              {result?.kind === "failed" && <span className="shrink-0 text-danger">Failed</span>}
              {result?.kind === "not_reached" && (
                <span className="shrink-0 text-faint">Not reached</span>
              )}
            </li>
          );
        })}
      </ol>
    </>
  );
}
