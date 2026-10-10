// Driving a whole selection unattended: the app opens its own browser per
// case, works through the script alone, and proposes a verdict. Nobody is
// watching by default - the person can leave and come back once it says
// "case N of M" is done.
//
// Nothing here is a verdict. `proposed` is the machine's best guess at what
// a person would have picked; it becomes a real verdict only once someone
// reviews it, and only a reviewed run can ever reach Azure DevOps.
//
// The run itself lives in lib/backgroundRun, not here: this window is only
// a view of it. It is mounted once, at App level, and shows whenever the
// store says the window is open, so Run in background (or Escape) closes
// it without touching the run, and the title-bar pill opens it again from
// any section.

import { useEffect, useId, useRef, useState } from "react";
import { commands, type PlanView, type StepRecord } from "../../bindings";
import SharedStepLabel from "../../components/SharedStepLabel";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import { Select } from "../../components/ui/select";
import { toast } from "../../lib/toast";
import {
  IconCancel,
  IconFollowRun,
  IconHideSteps,
  IconRunInBackground,
  IconShowSteps,
  IconStepDone,
  IconStop,
  IconUnattended,
} from "../../lib/actionIcons";
import {
  answerRunReset,
  closeRunWindow,
  sendRunToBackground,
  startRun,
  stopRun,
  useBackgroundRun,
  type BackgroundRun,
  type WrittenStep,
} from "../../lib/backgroundRun";
import { cn } from "../../lib/cn";
import { DISCOVERY_BUSY, useDiscoveryActiveNow } from "../../lib/discoveryActive";
import { dbReadAccessOn } from "../../lib/mcpTools";
import { useMediaQuery } from "../../lib/useMediaQuery";
import { resetLines } from "./plan";
import ResetNeededPanel from "./ResetNeededPanel";

export { statusOf, type WrittenStep } from "../../lib/backgroundRun";

// Same two browsers, same values, as the supervised pane's picker - and the
// same storage key, so a person's choice there is their choice here too.
const BROWSERS = [
  { value: "edge", label: "Microsoft Edge" },
  { value: "chrome", label: "Google Chrome" },
];

/** Where the Retry transient failures once choice is remembered. */
const RETRY_KEY = "tcm-v2-autorun-retry-transient";

/** A window this tall has room to show the case after the running one as
 * well, so it opens too. Shorter, only the running case opens. */
const TALL_WINDOW = "(min-height: 900px)";

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

/** Each phase with its case count, and a reset line at each boundary. */
function PlanSummary({ plan, cases }: { plan: PlanView; cases: { id: number; title: string }[] }) {
  const titleOf = (id: number) => cases.find((c) => c.id === id)?.title;
  return (
    <div aria-label="Run plan" className="space-y-1 rounded-md border border-border bg-surface-2 p-2 text-xs">
      {plan.phases.map((ids, i) => {
        const lines = i === 0 ? [] : plan.resets.filter((r) => r.before_case_id === ids[0]).flatMap((r) => resetLines(r, titleOf));
        return (
          <div key={ids[0]} className="space-y-1">
            {lines.map((l) => (
              <p key={l} className="border-l-2 border-l-warning pl-2 text-warning">{l}</p>
            ))}
            <p className="text-text">
              Phase {i + 1}: {ids.length} {ids.length === 1 ? "case" : "cases"}
            </p>
          </div>
        );
      })}
    </div>
  );
}

/** The run window, mounted once at App level. Shows while the store says
 * it is open; everything it shows of a run comes from the store. */
export default function ReplayPane() {
  const { run } = useBackgroundRun();
  if (!run || !run.open) return null;
  // Keyed on the scope: the remembered account is per organisation and
  // project, so another scope's setup starts from its own.
  return <RunWindow key={`${run.org}/${run.project}`} run={run} />;
}

function RunWindow({ run }: { run: BackgroundRun }) {
  const { org, project, cases, plan, phase } = run;
  const { rows, phases, latest, position, records, resetNeeded } = run;
  /** A discovery that began after this setup opened holds the browser:
   * Start waits for it, as the store does. */
  const discovering = useDiscoveryActiveNow();

  /** Same key the supervised pane uses - a stored value the picker cannot
   * show falls back to Edge, same reasoning as there. */
  const [browserName, setBrowserName] = useState(() => {
    const saved = localStorage.getItem("tcm-v2-autorun-browser");
    return BROWSERS.some((b) => b.value === saved) ? (saved as string) : "edge";
  });
  /** Off by default: an unattended run's whole point is that nobody has to
   * sit in front of it. */
  const [watch, setWatch] = useState(() => localStorage.getItem("tcm-v2-autorun-watch") === "1");
  /** Pause before each action: the short pause before a click, fill or drag
   * (the outline is always drawn). Kept by Rust (it also covers the supervised browser), on until
   * the saved setting says otherwise. */
  const [highlight, setHighlight] = useState(true);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const s = await commands.getAppSettings();
        if (live && s && typeof s.autorun_highlight === "boolean") setHighlight(s.autorun_highlight);
      } catch {
        // unavailable - the box shows its default
      }
    })();
    return () => {
      live = false;
    };
  }, []);
  const chooseHighlight = (on: boolean) => {
    setHighlight(on);
    void (async () => {
      try {
        const r = await commands.setAutorunHighlight(on);
        if (r.status === "error") throw r.error;
      } catch (e) {
        setHighlight(!on);
        toast.error(String(e));
      }
    })();
  };
  /** On by default: a case that failed in a way that looks transient (a
   * gateway error, a dropped connection, the browser going silent) runs
   * once more, in a fresh browser, and is labelled Retried. */
  const [retryTransient, setRetryTransient] = useState(() => {
    try {
      return localStorage.getItem(RETRY_KEY) !== "0";
    } catch {
      return true;
    }
  });

  /** The tester's accounts, key and name only: the Sign in as choices. */
  const [accounts, setAccounts] = useState<{ key: string; label: string }[]>([]);
  const settingUp = phase === "setup" || phase === "failed";
  useEffect(() => {
    if (!settingUp) return;
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
  }, [settingUp]);
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

  // A case starting is locked open and a finished one folds: either way the
  // person's own choice for it is spent. Each progress event is a new
  // `latest`, so this runs once per event.
  useEffect(() => {
    if (!latest) return;
    setByHand((prev) => {
      if (!(latest.caseId in prev)) return prev;
      const { [latest.caseId]: _spent, ...rest } = prev;
      return rest;
    });
  }, [latest]);

  const start = () => {
    setByHand({});
    setPaused(false);
    void startRun({
      account: runAccount || null,
      browserName,
      watch,
      retryTransient,
      // Preconditions follow Database Read Access: off, none is checked.
      dbReadAccess: dbReadAccessOn(),
    });
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
  // of case scrolls, and never while the person has the scroll. A window
  // opened again from the title bar starts on the running case too.
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

  // Escape and a backdrop click: a run going is sent to the background (it
  // carries on, and the title-bar pill brings it back), a setup is
  // cancelled. Only Stop ends a run.
  return (
    <Modal onClose={closeRunWindow} className={`${MODAL_LARGE} space-y-3 overflow-y-auto p-4`}>
      <h2 className="text-sm font-semibold text-text">Unattended run</h2>

      {settingUp ? (
        <div className="space-y-3">
          {run.error && <p className="text-xs text-danger">{run.error}</p>}
          {plan && plan.phases.length > 1 && <PlanSummary plan={plan} cases={cases} />}
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
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox checked={highlight} ariaLabel="Pause before each action" onCheckedChange={chooseHighlight} />
            Pause before each action
          </label>
          <p className="text-xs text-faint">
            Off: the browser runs in the background and you can keep working. On: a window opens
            for every case.
          </p>
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox
              checked={retryTransient}
              ariaLabel="Retry transient failures once"
              onCheckedChange={(on) => {
                setRetryTransient(on);
                try {
                  localStorage.setItem(RETRY_KEY, on ? "1" : "0");
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            />
            Retry transient failures once
          </label>
          <p className="text-xs text-faint">
            A case that fails on a gateway error, a dropped connection or a browser that stops
            answering runs once more, and is labelled Retried.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={closeRunWindow}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              size="sm"
              className="disabled:pointer-events-auto"
              disabled={discovering}
              title={discovering ? DISCOVERY_BUSY : undefined}
              onClick={start}
            >
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
          {resetNeeded && (
            <ResetNeededPanel
              reset={resetNeeded}
              remaining={resetNeeded.remaining}
              titleOf={(id) => cases.find((c) => c.id === id)?.title}
              busy={run.answering}
              onContinue={() => void answerRunReset(true)}
              onStop={() => void answerRunReset(false)}
            />
          )}
          {/* Only the person's own input pauses following - a bare `scroll`
              event is also what scrollIntoView itself causes. */}
          <ul
            aria-label="Cases in this run"
            className="max-h-[55vh] space-y-1 overflow-y-auto"
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
          {/* Leaving on the left, ending on the right: Run in background
              closes the window and the run carries on; only Stop ends it. */}
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                title="Close this window. The run carries on, and the title bar shows how it is going."
                onClick={sendRunToBackground}
              >
                <IconRunInBackground aria-hidden />
                Run in background
              </Button>
              {paused && (
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
              )}
            </div>
            <Button size="sm" variant="outline" disabled={run.stopping} onClick={() => void stopRun()}>
              <IconStop aria-hidden />
              {run.stopping ? "Stopping after this step" : "Stop"}
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
