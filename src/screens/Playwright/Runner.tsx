// The Playwright tab's Runner: the repo's tests as a tree, run the way
// `npm run qa` runs them. Rust writes the run list, dry-counts it and refuses
// a run whose count disagrees (pw_repo::runner); this screen picks, shows the
// output as it streams, stops, and lists the reports.
//
// A toolbar of run actions and the browser mode; then the tree and, beside
// it on a wide window, the output console (its own Stop, following new lines
// while it is scrolled to the bottom) and the reports.
//
// The ticks are remembered per clone (lib/cache, `cacheKeys.pwTicks`).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, events, type BrowserMode, type RunAction, type RunPreview } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Select } from "../../components/ui/select";
import {
  IconCancel,
  IconClear,
  IconConfirm,
  IconOpenInBrowser,
  IconRefresh,
  IconReview,
  IconRun,
  IconSetDefault,
  IconStop,
} from "../../lib/actionIcons";
import { CACHE, cacheKeys, cacheRead, cacheWrite } from "../../lib/cache";
import { unwrapStr } from "../../lib/ipc";
import { Card, Hint } from "./parts";
import RunnerTree, { buildBranches } from "./RunnerTree";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query keys of the clone's tests and reports; the folder is part of each. */
export const TESTS_KEY = "pw-tests";
export const REPORTS_KEY = "pw-reports";
/** The most output lines the panel keeps; older ones fall off the top. */
export const OUTPUT_CAP = 5000;
/** The most ends of other runs kept, for a run whose start comes back late. */
export const ENDED_CAP = 20;
/** After a listing or dry count ends, how often and how many times to look
 * for the run it prepared. */
const ADOPT_RETRY_MS = 250;
const ADOPT_TRIES = 8;

/** Runs `f` at the next frame (a timer where there are no frames); the
 * answer cancels it. */
function nextFrame(f: () => void): () => void {
  if (typeof requestAnimationFrame === "function") {
    const id = requestAnimationFrame(f);
    return () => cancelAnimationFrame(id);
  }
  const id = setTimeout(f, 16);
  return () => clearTimeout(id);
}

/** `lines` with only the newest OUTPUT_CAP kept. */
const capped = (lines: string[]) => (lines.length > OUTPUT_CAP ? lines.slice(lines.length - OUTPUT_CAP) : lines);

/** The debug question: the run it is about, in the mode it was asked in. */
type Ask = { action: RunAction; count: number; mode: BrowserMode };

function readTicks(clone: string): Set<string> {
  return new Set(cacheRead<string[]>(cacheKeys.pwTicks(clone), CACHE.picks.ttlMs) ?? []);
}

function stats(s: { passed: number; failed: number; flaky: number; skipped: number } | null) {
  if (!s) return "";
  return [`${s.passed} passed`, `${s.failed} failed`, s.flaky ? `${s.flaky} flaky` : "", s.skipped ? `${s.skipped} skipped` : ""]
    .filter(Boolean)
    .join(", ");
}

export default function Runner({ clone, onBusy }: { clone: string; onBusy?: (busy: boolean) => void }) {
  const qc = useQueryClient();
  const tests = useQuery({
    queryKey: [TESTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwList(clone, false)),
    // A failed listing is a repo command that failed: running it again
    // unasked would only take the run slot again. List again is the retry.
    retry: false,
    // Kept until something says it changed: a run's end, List again,
    // Refresh, an export or a Sync write.
    staleTime: Infinity,
  });
  const reports = useQuery({
    queryKey: [REPORTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwReports(clone)),
    retry: false,
    // A run's end reads them again (`fetchReports`); Refresh does too.
    staleTime: Infinity,
  });
  const fetchReports = () => qc.fetchQuery({ queryKey: [REPORTS_KEY, clone], queryFn: () => unwrapStr(commands.pwReports(clone)), retry: false });

  const [ticked, setTicked] = useState<Set<string>>(() => readTicks(clone));
  const [mode, setMode] = useState<BrowserMode>("Hidden");
  /** A list, preview or start in flight. */
  const [busy, setBusy] = useState(false);
  /** The run started, until its end. */
  const [running, setRunning] = useState<number | null>(null);
  /** A listing or dry count going when this screen opened: not this screen's
   * to adopt, but it is busy until that ends. */
  const [waitingOn, setWaitingOn] = useState<number | null>(null);
  const [output, setOutput] = useState<string[]>([]);
  /** Lines come in bursts: they wait here and reach the panel once per
   * frame, so a busy run is one render a frame, not one a line. */
  const pending = useRef<string[]>([]);
  const cancelFlush = useRef<(() => void) | null>(null);
  const addLine = (line: string) => {
    pending.current = capped([...pending.current, line]);
    cancelFlush.current ??= nextFrame(() => {
      cancelFlush.current = null;
      const lines = pending.current;
      pending.current = [];
      setOutput((o) => capped([...o, ...lines]));
    });
  };
  /** Empties the panel, and drops any line still waiting for its frame. */
  const clearOutput = () => {
    cancelFlush.current?.();
    cancelFlush.current = null;
    pending.current = [];
    setOutput([]);
  };
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [preview, setPreview] = useState<RunPreview | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  /** The output panel, and whether it follows new lines (it does until the
   * person scrolls up from the bottom). */
  const logRef = useRef<HTMLPreElement>(null);
  const follow = useRef(true);
  useEffect(() => {
    const el = logRef.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [output]);

  // What the event listeners read: the latest values, not the first render's.
  // `ended` holds the end of a run whose start has not come back yet: a run
  // that fails at once can end before `pw_run` answers.
  const live = useRef({
    busy,
    running,
    waitingOn,
    reportsBefore: new Set<string>(),
    ended: new Map<number, number | null>(),
  });
  live.current.busy = busy;
  live.current.running = running;
  live.current.waitingOn = waitingOn;

  /** Keep only ends newer than `run` (and at most ENDED_CAP of them). */
  const prune = (run: number) => {
    const ended = live.current.ended;
    for (const id of [...ended.keys()]) if (id <= run) ended.delete(id);
    while (ended.size > ENDED_CAP) ended.delete(ended.keys().next().value as number);
  };

  /** This run is this screen's now. */
  const adoptRun = (run: number) => {
    prune(run);
    live.current.running = run;
    setRunning(run);
  };

  /**
   * Take on the repo command going now, if it is a test run. A listing or dry
   * count (brief) is waited out instead, and looked at again when it ends:
   * the run it was preparing starts right after. `tries` re-asks a moment
   * later while nothing is going yet.
   */
  const adopt = useRef(async (_tries: number) => {});
  adopt.current = async (tries) => {
    try {
      const now = await unwrapStr(commands.pwRepoRunning());
      const l = live.current;
      if (l.busy || l.running !== null) return;
      if (!now) {
        if (tries > 0) setTimeout(() => void adopt.current(tries - 1), ADOPT_RETRY_MS);
        return;
      }
      if (!now.label.startsWith("npx playwright") || l.ended.has(now.run)) return;
      if (now.brief) {
        l.waitingOn = now.run;
        setWaitingOn(now.run);
        return;
      }
      l.reportsBefore = new Set((await fetchReports()).map((r) => r.run_id));
      if (l.busy || l.running !== null || l.ended.has(now.run)) return;
      adoptRun(now.run);
    } catch {
      // Nothing to adopt.
    }
  };

  /** The run ended: say how, show the results, and open its report. */
  const finish = useRef((_code: number | null) => {});
  finish.current = (code) => {
    setRunning(null);
    live.current.running = null;
    setNote(code === null ? "The run was stopped." : code === 0 ? "The run passed." : `The run ended with failures (exit ${code}).`);
    const before = live.current.reportsBefore;
    void (async () => {
      void qc.invalidateQueries({ queryKey: [TESTS_KEY, clone] });
      try {
        const now = await fetchReports();
        // Only a report this run made: one that was not listed before it.
        const fresh = now.find((r) => !before.has(r.run_id));
        if (fresh) await unwrapStr(commands.pwOpenReport(clone, fresh.run_id));
      } catch (err) {
        setError(message(err));
      }
    })();
  };

  // Stable, so the tree (memo) renders again only when the ticks or the
  // tests change, never for a line of output.
  const tick = useCallback(
    (ids: string[], on: boolean) => {
      setTicked((t) => {
        const next = new Set(t);
        for (const id of ids) {
          if (on) next.add(id);
          else next.delete(id);
        }
        cacheWrite(cacheKeys.pwTicks(clone), [...next]);
        return next;
      });
    },
    [clone],
  );

  const list = useMemo(() => tests.data?.tests ?? [], [tests.data]);
  const loadErrors = tests.data?.load_errors ?? [];
  const branches = useMemo(() => buildBranches(list), [list]);
  const known = useMemo(() => new Set(list.map((t) => t.id)), [list]);
  const picked = [...ticked].filter((id) => known.has(id));
  const active = busy || running !== null || waitingOn !== null;
  // The screen around says so while a command of this Runner is going.
  useEffect(() => {
    onBusy?.(active);
  }, [onBusy, active]);
  useEffect(() => () => onBusy?.(false), [onBusy]);

  useEffect(() => {
    const output = events.pwOutput.listen((e) => {
      // Once the run's id is known, only its lines. Before that (the listing
      // and the dry count, whose ids are not exposed) only while this
      // screen is itself starting something.
      const { busy, running, waitingOn } = live.current;
      const ours = running !== null ? e.payload.run === running : e.payload.run === waitingOn || busy;
      if (!ours) return;
      addLine(e.payload.line);
    });
    let gone = false;
    const done = events.pwDone.listen((e) => {
      const { run, code } = e.payload;
      if (run === live.current.running) finish.current(code);
      else if (run === live.current.waitingOn) {
        // The listing or dry count ended: the run it prepared, if any, is
        // about to start. Look again, for a moment.
        live.current.waitingOn = null;
        setWaitingOn(null);
        if (!gone) void adopt.current(ADOPT_TRIES);
      } else {
        live.current.ended.set(run, code);
        prune(live.current.running ?? 0);
      }
    });
    // A run started before this screen was (the tab was left and come back
    // to) is still this Runner's: Stop works, the actions wait, and its
    // output and end arrive here.
    void adopt.current(0);
    return () => {
      gone = true;
      cancelFlush.current?.();
      cancelFlush.current = null;
      for (const un of [output, done]) un.then((f) => f()).catch(() => {});
    };
    // Mount only: `live` and `finish` carry the current values.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const begin = () => {
    setBusy(true);
    setError("");
    setNote("");
    setWarnings([]);
    setPreview(null);
    setAsk(null);
  };

  const refresh = async () => {
    begin();
    clearOutput();
    try {
      qc.setQueryData([TESTS_KEY, clone], await unwrapStr(commands.pwList(clone, true)));
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const run = async (action: RunAction, confirm = false, as: BrowserMode = mode) => {
    begin();
    clearOutput();
    try {
      // What was listed before the run, so its end opens only its own report.
      const before = reports.data ?? (await fetchReports());
      live.current.reportsBefore = new Set(before.map((r) => r.run_id));
      const r = await unwrapStr(commands.pwRun(clone, picked, action, as, confirm));
      if (r.kind === "ConfirmDebug") {
        setAsk({ action, count: r.count, mode: as });
        return;
      }
      setWarnings(r.warnings);
      setOutput((o) => [`$ ${r.command}`, r.summary, ...o]);
      const ended = live.current.ended;
      if (ended.has(r.run)) {
        const code = ended.get(r.run) ?? null;
        prune(r.run);
        finish.current(code);
      } else adoptRun(r.run);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const showPreview = async (action: RunAction) => {
    begin();
    try {
      const p = await unwrapStr(commands.pwPreview(clone, picked, action, mode));
      setPreview(p);
      setWarnings(p.warnings);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    try {
      const now = running ?? waitingOn ?? (await unwrapStr(commands.pwRepoRunning()))?.run ?? null;
      if (now !== null) await unwrapStr(commands.pwRepoStop(now));
    } catch (e) {
      setError(message(e));
    }
  };

  const open = async (runId: string) => {
    setError("");
    try {
      await unwrapStr(commands.pwOpenReport(clone, runId));
    } catch (e) {
      setError(message(e));
    }
  };

  const working = running !== null || waitingOn !== null || busy;
  const status = running !== null ? "Running…" : working ? "Working…" : note;
  const statusTone = working
    ? "text-accent"
    : running === null && note === "The run passed."
      ? "text-success"
      : note.startsWith("The run ended with failures")
        ? "text-danger"
        : "text-muted";

  return (
    // Side by side (wide windows) the two columns fill the tab's height and
    // end on one line, the tests and the output scrolling inside; stacked,
    // each keeps its own height and the tab scrolls.
    <section className="flex flex-col gap-3 min-[1400px]:h-full" aria-label="Runner">
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-surface p-2">
        <Button size="sm" disabled={active || picked.length === 0} onClick={() => void run("Picked")}>
          <IconRun aria-hidden />
          {`Run picked (${picked.length})`}
        </Button>
        <Button size="sm" variant="outline" disabled={active || list.length === 0} onClick={() => void run("All")}>
          <IconRun aria-hidden />
          Run all
        </Button>
        <Button size="sm" variant="outline" disabled={active} onClick={() => void run("Failed")}>
          <IconRefresh aria-hidden />
          Re-run failures
        </Button>
        <Button size="sm" variant="outline" disabled={active || picked.length === 0} onClick={() => void showPreview("Picked")}>
          <IconReview aria-hidden />
          Preview
        </Button>
        <span className="mx-1 h-6 w-px bg-border" aria-hidden />
        <Select
          aria-label="Browser"
          className="w-40"
          triggerClassName="py-1.5 text-xs"
          value={mode}
          disabled={active || ask !== null}
          onChange={(e) => setMode(e.target.value as BrowserMode)}
        >
          <option value="Hidden">Browser hidden</option>
          <option value="Shown">Browser shown</option>
          <option value="Debug">Debug</option>
        </Select>
      </div>

      {ask && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-warning/40 bg-warning/10 p-2" role="alert">
          <span className="min-w-0 flex-1 text-sm text-text">
            {`Debug pauses at the start of every test, and this runs ${ask.count}. Run them all in debug mode?`}
          </span>
          <Button size="sm" onClick={() => void run(ask.action, true, ask.mode)}>
            <IconConfirm aria-hidden />
            Run in debug
          </Button>
          <Button size="sm" variant="outline" onClick={() => setAsk(null)}>
            <IconCancel aria-hidden />
            Cancel
          </Button>
        </div>
      )}
      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
      {warnings.map((w) => (
        <p key={w} className="text-xs text-warning">
          {w}
        </p>
      ))}

      <div className="grid gap-3 min-[1400px]:min-h-0 min-[1400px]:flex-1 min-[1400px]:grid-cols-2">
        <Card
          className="min-[1400px]:flex min-[1400px]:min-h-0 min-[1400px]:flex-col"
          title={
            <>
              Tests <span className="text-xs font-normal text-faint">{list.length}</span>
            </>
          }
          actions={
            <div className="flex items-center gap-1">
              <Button size="sm" variant="ghost" disabled={list.length === 0} onClick={() => tick(list.map((t) => t.id), true)}>
                <IconSetDefault aria-hidden />
                Tick all
              </Button>
              <Button size="sm" variant="ghost" disabled={picked.length === 0} onClick={() => tick([...ticked], false)}>
                <IconClear aria-hidden />
                Clear
              </Button>
              <Button size="sm" variant="ghost" disabled={active || tests.isFetching} onClick={() => void refresh()}>
                <IconRefresh aria-hidden />
                List again
              </Button>
            </div>
          }
          bodyClassName="max-h-[34rem] overflow-auto p-2 min-[1400px]:max-h-none min-[1400px]:min-h-0 min-[1400px]:flex-1"
        >
          {tests.isLoading && <Hint className="p-2">Listing the tests…</Hint>}
          {tests.isError && <p className="whitespace-pre-wrap p-2 text-xs text-danger">{tests.error.message}</p>}
          {tests.isSuccess && loadErrors.length > 0 && (
            // Playwright lists no test at all when one file cannot load, so
            // "no tests" would send the person looking in the wrong place.
            <div role="alert" className="space-y-1 p-2 text-xs text-danger">
              <p>Playwright could not load the tests, so none are listed:</p>
              <ul className="list-disc space-y-0.5 pl-4">
                {loadErrors.map((e) => (
                  <li key={e} className="whitespace-pre-wrap break-words">
                    {e}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {tests.isSuccess && list.length === 0 && loadErrors.length === 0 && (
            <Hint className="p-2">No tests under suites yet.</Hint>
          )}
          {branches.length > 0 && <RunnerTree branches={branches} ticked={ticked} onTick={tick} />}
        </Card>

        <div className="flex min-w-0 flex-col gap-3 min-[1400px]:min-h-0">
          {preview && (
            <Card title="Preview" aria-label="Preview" className="shrink-0" bodyClassName="space-y-1 p-3">
              <Hint>{`Would run ${preview.expected}:`}</Hint>
              <pre className="overflow-x-auto rounded-md bg-surface-2 p-2 font-mono text-xs text-text">
                {`${preview.run_list ?? ""}${preview.run_list ? "\n" : ""}${preview.command}`}
              </pre>
            </Card>
          )}

          <Card
            title="Output"
            className="min-[1400px]:flex min-[1400px]:min-h-0 min-[1400px]:flex-1 min-[1400px]:flex-col"
            bodyClassName="min-[1400px]:flex min-[1400px]:min-h-0 min-[1400px]:flex-1 min-[1400px]:flex-col"
            actions={
              <>
                {status && <span className={`text-xs ${statusTone}`}>{status}</span>}
                <Button size="sm" variant="outline" disabled={!active} onClick={() => void stop()}>
                  <IconStop aria-hidden />
                  Stop
                </Button>
              </>
            }
          >
            <pre
              ref={logRef}
              role="log"
              aria-label="Run output"
              onScroll={(e) => {
                const el = e.currentTarget;
                follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
              }}
              className="h-72 overflow-auto bg-bg p-3 font-mono text-xs leading-relaxed text-text min-[1400px]:h-auto min-[1400px]:min-h-40 min-[1400px]:flex-1"
            >
              {output.length > 0 ? (
                output.join("\n")
              ) : (
                <span className="text-faint">{active ? "Starting…" : "No run yet. Output appears here as a run goes."}</span>
              )}
            </pre>
          </Card>

          <Card title="Reports" aria-label="Reports" className="shrink-0" bodyClassName="max-h-64 overflow-auto">
            {reports.isError && <p className="p-3 text-xs text-danger">{reports.error.message}</p>}
            {reports.data && reports.data.length === 0 && <Hint className="p-3">No report yet. Run something first.</Hint>}
            <ul className="divide-y divide-border">
              {(reports.data ?? []).map((r) => (
                <li key={r.run_id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-1.5">
                  <span className="font-mono text-xs text-text">{r.run_id.replace("T", " ").replace(/-(\d\d)-(\d\d)$/, ":$1:$2")}</span>
                  <span
                    className={`min-w-0 flex-1 text-xs ${r.stats?.failed ? "text-danger" : r.stats ? "text-success" : "text-muted"}`}
                  >
                    {stats(r.stats)}
                  </span>
                  <Button size="sm" variant="ghost" aria-label={`Open the report of ${r.run_id}`} onClick={() => void open(r.run_id)}>
                    <IconOpenInBrowser aria-hidden />
                    Open
                  </Button>
                </li>
              ))}
            </ul>
          </Card>
        </div>
      </div>
    </section>
  );
}
