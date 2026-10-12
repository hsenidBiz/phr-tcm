// The Playwright tab's Runner: the repo's tests as a tree, run the way
// `npm run qa` runs them. Rust writes the run list, dry-counts it and refuses
// a run whose count disagrees (pw_repo::runner); this screen picks, shows the
// output as it streams, stops, and lists the reports.
//
// The ticks are remembered per clone (lib/cache, `cacheKeys.pwTicks`).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
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
import RunnerTree, { buildBranches } from "./RunnerTree";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query keys of the clone's tests and reports; the folder is part of each. */
export const TESTS_KEY = "pw-tests";
export const REPORTS_KEY = "pw-reports";
/** The most output lines the panel keeps; older ones fall off the top. */
export const OUTPUT_CAP = 5000;

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

export default function Runner({ clone }: { clone: string }) {
  const qc = useQueryClient();
  const tests = useQuery({
    queryKey: [TESTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwList(clone, false)),
    // A failed listing is a repo command that failed: running it again
    // unasked would only take the run slot again. List again is the retry.
    retry: false,
  });
  const reports = useQuery({
    queryKey: [REPORTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwReports(clone)),
    retry: false,
  });
  const fetchReports = () => qc.fetchQuery({ queryKey: [REPORTS_KEY, clone], queryFn: () => unwrapStr(commands.pwReports(clone)), retry: false });

  const [ticked, setTicked] = useState<Set<string>>(() => readTicks(clone));
  const [mode, setMode] = useState<BrowserMode>("Hidden");
  /** A list, preview or start in flight. */
  const [busy, setBusy] = useState(false);
  /** The run started, until its end. */
  const [running, setRunning] = useState<number | null>(null);
  const [output, setOutput] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [note, setNote] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [preview, setPreview] = useState<RunPreview | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);

  // What the event listeners read: the latest values, not the first render's.
  // `ended` holds the end of a run whose start has not come back yet: a run
  // that fails at once can end before `pw_run` answers.
  const live = useRef({ busy, running, reportsBefore: new Set<string>(), ended: new Map<number, number | null>() });
  live.current.busy = busy;
  live.current.running = running;

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

  const tick = (ids: string[], on: boolean) => {
    setTicked((t) => {
      const next = new Set(t);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      cacheWrite(cacheKeys.pwTicks(clone), [...next]);
      return next;
    });
  };

  const list = tests.data?.tests ?? [];
  const branches = useMemo(() => buildBranches(list), [list]);
  const known = useMemo(() => new Set(list.map((t) => t.id)), [list]);
  const picked = [...ticked].filter((id) => known.has(id));
  const active = busy || running !== null;

  useEffect(() => {
    const output = events.pwOutput.listen((e) => {
      // Once the run's id is known, only its lines. Before that (the listing
      // and the dry count, whose ids are not exposed) only while this
      // screen is itself starting something.
      const { busy, running } = live.current;
      if (running !== null ? e.payload.run !== running : !busy) return;
      setOutput((o) => {
        const next = [...o, e.payload.line];
        return next.length > OUTPUT_CAP ? next.slice(next.length - OUTPUT_CAP) : next;
      });
    });
    const done = events.pwDone.listen((e) => {
      if (e.payload.run === live.current.running) finish.current(e.payload.code);
      else live.current.ended.set(e.payload.run, e.payload.code);
    });
    // A run started before this screen was (the tab was left and come back
    // to) is still this Runner's: Stop works, the actions wait, and its
    // output and end arrive here.
    let gone = false;
    void (async () => {
      try {
        const now = await unwrapStr(commands.pwRepoRunning());
        if (gone || !now || !now.label.startsWith("npx playwright")) return;
        if (live.current.busy || live.current.running !== null || live.current.ended.has(now.run)) return;
        live.current.reportsBefore = new Set((await fetchReports()).map((r) => r.run_id));
        if (gone || live.current.ended.has(now.run)) return;
        live.current.running = now.run;
        setRunning(now.run);
      } catch {
        // Nothing to adopt.
      }
    })();
    return () => {
      gone = true;
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
    setOutput([]);
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
    setOutput([]);
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
      if (ended.has(r.run)) finish.current(ended.get(r.run) ?? null);
      else {
        setRunning(r.run);
        live.current.running = r.run;
      }
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
      const now = running ?? (await unwrapStr(commands.pwRepoRunning()))?.run ?? null;
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

  return (
    <section className="space-y-3" aria-label="Runner">
      <h3 className={sectionTitle}>Runner</h3>

      <div className="flex flex-wrap items-center gap-2">
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
        <Select
          aria-label="Browser"
          value={mode}
          disabled={active || ask !== null}
          onChange={(e) => setMode(e.target.value as BrowserMode)}
        >
          <option value="Hidden">Browser hidden</option>
          <option value="Shown">Browser shown</option>
          <option value="Debug">Debug</option>
        </Select>
        <Button size="sm" variant="outline" disabled={!active} onClick={() => void stop()}>
          <IconStop aria-hidden />
          Stop
        </Button>
      </div>

      {ask && (
        <div className="flex flex-wrap items-center gap-2 rounded border border-border bg-surface p-2" role="alert">
          <span className="text-sm text-text">
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
      {error && <p className="text-xs text-danger">{error}</p>}
      {note && <p className="text-xs text-muted">{note}</p>}
      {warnings.map((w) => (
        <p key={w} className="text-xs text-warning">
          {w}
        </p>
      ))}

      {preview && (
        <div className="space-y-1" aria-label="Preview">
          <p className="text-xs text-muted">{`Would run ${preview.expected}:`}</p>
          <pre className="overflow-x-auto rounded border border-border bg-surface p-2 text-xs text-text">
            {`${preview.run_list ?? ""}${preview.run_list ? "\n" : ""}${preview.command}`}
          </pre>
        </div>
      )}

      {(output.length > 0 || active) && (
        <pre
          role="log"
          aria-label="Run output"
          className="max-h-80 overflow-auto rounded border border-border bg-surface p-2 text-xs text-text"
        >
          {output.join("\n")}
        </pre>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="outline" disabled={list.length === 0} onClick={() => tick(list.map((t) => t.id), true)}>
          <IconSetDefault aria-hidden />
          Tick all
        </Button>
        <Button size="sm" variant="outline" disabled={picked.length === 0} onClick={() => tick([...ticked], false)}>
          <IconClear aria-hidden />
          Clear
        </Button>
        <Button size="sm" variant="outline" disabled={active || tests.isFetching} onClick={() => void refresh()}>
          <IconRefresh aria-hidden />
          List again
        </Button>
      </div>
      {tests.isLoading && <p className="text-xs text-muted">Listing the tests…</p>}
      {tests.isError && <p className="text-xs text-danger">{tests.error.message}</p>}
      {tests.isSuccess && list.length === 0 && <p className="text-xs text-muted">No tests under suites yet.</p>}
      {branches.length > 0 && <RunnerTree branches={branches} ticked={ticked} onTick={tick} />}

      <div className="space-y-1" aria-label="Reports">
        <h4 className={sectionTitle}>Reports</h4>
        {reports.isError && <p className="text-xs text-danger">{reports.error.message}</p>}
        {reports.data && reports.data.length === 0 && <p className="text-xs text-muted">No report yet. Run something first.</p>}
        <ul className="space-y-1">
          {(reports.data ?? []).map((r) => (
            <li key={r.run_id} className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-text">{r.run_id.replace("T", " ").replace(/-(\d\d)-(\d\d)$/, ":$1:$2")}</span>
              <span className="text-xs text-muted">{stats(r.stats)}</span>
              <Button size="sm" variant="outline" aria-label={`Open the report of ${r.run_id}`} onClick={() => void open(r.run_id)}>
                <IconOpenInBrowser aria-hidden />
                Open
              </Button>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
