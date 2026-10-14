// The Playwright tab's Sync: whether the clone's copy of each exported Auto
// Run script is current, and Update, which exports the chosen cases again
// through Export to Playwright's own all-or-nothing write (pw_repo::sync).
//
// Each case is one of: current, out of date (its script changed after the
// export, and what changed), not in the repo (its section or raw spec is
// gone), never exported, or in the repo but not from this app (the repo has
// it and this app has no export of it on record). A script change anywhere
// in the app (`autorunScriptsChanged`) reads the states again.
//
// Add new exports every case with a script that the repo has nowhere; Rust
// works out which (`sync::add_new`). Update out of date and Add new never
// touch a case the repo has that this app did not export; Update ticked
// replaces one only when it is ticked, and sends it as chosen to replace.
//
// Update replaces a case's raw spec and rewrites its section in the
// test-case file from Azure DevOps; the repo's Added by engineer, Also and
// Assertion floor blocks, every other section and every other file are left
// as they are. A raw spec still needs the repo's refactorer, which each
// updated case says, and Finish in the repo gives the prompt for it.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { commands, events, type ExportResult, type SyncRow, type SyncState } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { IconAdd, IconExport, IconRefresh } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import FinishInRepo from "./FinishInRepo";
import { Card, Hint, StatusBadge, type Tone } from "./parts";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query key of the states; the clone, org and project are part of it. */
export const SYNC_KEY = "pw-sync";

/** How many Sync screens are open, and whether a script changed while none
 * was: the states are kept (`staleTime: Infinity`), so the next Sync to open
 * reads them again. Watched from the first Sync on, which is when there is
 * first anything kept. */
let syncMounted = 0;
let staleWhileAway = false;
let awayWatch: Promise<unknown> | null = null;
function watchWhileAway() {
  if (awayWatch) return;
  awayWatch = events.autorunScriptsChanged
    .listen(() => {
      if (syncMounted === 0) staleWhileAway = true;
    })
    .catch(() => {
      awayWatch = null;
    });
}

/** A UTC stamp as a local date and time; anything unreadable as it is. */
function localTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** What each updated case still needs in the repo: said here only. */
export const NEEDS_REPO = "needs /gen-test steps 4, 5, 6 and 7 in the repo (Finish in the repo, below)";

function stateText(s: SyncState): string {
  switch (s.kind) {
    case "Current":
      return "Current";
    case "OutOfDate":
      return "Out of date";
    case "NotInRepo":
      return "Not in the repo";
    case "NeverExported":
      return "Never exported";
    case "InRepoNotFromApp":
      return "In the repo (not from this app)";
    case "ExcludedInRepo":
      return "Excluded in the repo";
  }
}

function stateTone(s: SyncState): Tone {
  switch (s.kind) {
    case "Current":
      return "success";
    case "OutOfDate":
      return "warning";
    case "NotInRepo":
    case "InRepoNotFromApp":
      return "info";
    case "ExcludedInRepo":
      return "muted";
    case "NeverExported":
      return "neutral";
  }
}

export default function Sync({
  clone,
  org,
  project,
  onUpdated,
}: {
  clone: string;
  org: string;
  project: string;
  /** Called after an Update wrote the clone. */
  onUpdated: () => void;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const key = [SYNC_KEY, clone, org, project];
  const states = useQuery({
    queryKey: key,
    queryFn: () => unwrapStr(commands.pwSyncStates(clone, org, project)),
    enabled: Boolean(clone && org && project),
    // Kept until something says it changed: a write here, a script changed
    // (even while Sync was not open, `syncMounted`), Read again or Refresh.
    staleTime: Infinity,
  });
  const [ticked, setTicked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  /** The last write was Add new: an empty one says there was nothing new. */
  const [wasAddNew, setWasAddNew] = useState(false);
  /** What a write did shows under the list: brought into view when it comes. */
  const resultRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (result) resultRef.current?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  }, [result]);

  // A script saved, repaired or removed anywhere: its state may have changed.
  // One changed while no Sync was open makes the kept states stale too.
  useEffect(() => {
    syncMounted += 1;
    watchWhileAway();
    if (staleWhileAway) {
      staleWhileAway = false;
      void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
    }
    const un = events.autorunScriptsChanged.listen(() => {
      void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
    });
    return () => {
      syncMounted -= 1;
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);

  const rows: SyncRow[] = states.data ?? [];
  const outOfDate = rows.filter((r) => r.state.kind === "OutOfDate" && r.has_script).map((r) => r.case_id);
  // A case the team excluded in the repo is never written, so never ticked.
  const tickable = (r: SyncRow) => r.has_script && r.state.kind !== "ExcludedInRepo";
  const picked = rows.filter((r) => tickable(r) && ticked.has(r.case_id)).map((r) => r.case_id);
  // The same rule as Rust's `sync::add_new`, which picks the cases itself.
  const addNew = rows.filter((r) => r.has_script && !r.in_repo).length;

  /** Runs one write: Update (with what was ticked to replace) or Add new. */
  const write = async (work: () => Promise<ExportResult>, addNew = false) => {
    setWasAddNew(addNew);
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const r = await work();
      setResult(r);
      setTicked(new Set());
      onUpdated();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
      void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
    }
  };

  const refs = [prefs.moduleRef ?? null, prefs.preconditionsRef ?? null] as const;
  // Out of date cases are this app's own exports: nothing is sent to replace.
  const updateOutOfDate = () =>
    write(() => unwrapStr(commands.pwSyncUpdate(clone, org, project, outOfDate, [], ...refs)));
  // Ticking a case is choosing it: one the repo has that this app did not
  // export is replaced only this way.
  const updateTicked = () =>
    write(() => unwrapStr(commands.pwSyncUpdate(clone, org, project, picked, picked, ...refs)));
  const addNewCases = () => write(() => unwrapStr(commands.pwSyncAddNew(clone, org, project, ...refs)), true);
  const nothingNew = Boolean(
    result && wasAddNew && result.cases.length === 0 && (result.skipped ?? []).length === 0,
  );

  const tick = (id: number, on: boolean) =>
    setTicked((cur) => {
      const next = new Set(cur);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const th = "px-3 py-2 text-left text-xs font-medium text-muted";

  return (
    <section className="space-y-3" aria-label="Sync">
      <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-surface p-2">
        <Button size="sm" disabled={busy || outOfDate.length === 0} onClick={() => void updateOutOfDate()}>
          <IconExport aria-hidden />
          {`Update out of date (${outOfDate.length})`}
        </Button>
        <Button size="sm" variant="outline" disabled={busy || addNew === 0} onClick={() => void addNewCases()}>
          <IconAdd aria-hidden />
          {`Add new (${addNew})`}
        </Button>
        <Button size="sm" variant="outline" disabled={busy || picked.length === 0} onClick={() => void updateTicked()}>
          <IconExport aria-hidden />
          {`Update ticked (${picked.length})`}
        </Button>
        <span className="flex-1" />
        {busy && <span className="text-xs text-accent">Updating…</span>}
        <Button size="sm" variant="ghost" disabled={busy || states.isFetching} onClick={() => void states.refetch()}>
          <IconRefresh aria-hidden />
          Refresh
        </Button>
      </div>
      <div className="space-y-0.5">
        <Hint>
          Update exports a case again: its raw spec is replaced and its section rewritten from Azure DevOps; the repo's
          Added by engineer, Also and Assertion floor blocks and every other section and file stay as they are.
        </Hint>
        <Hint>
          Add new exports every case the repo does not have yet. A case the repo has that this app did not export is
          replaced only when you tick it and use Update ticked.
        </Hint>
      </div>
      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}

      {states.isLoading && <Hint>Reading what was exported…</Hint>}
      {states.isError && <p className="whitespace-pre-wrap text-xs text-danger">{states.error.message}</p>}
      {states.isSuccess && rows.length === 0 && <Hint>No scripts in this project yet.</Hint>}
      {rows.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-border bg-surface">
          <table className="w-full text-sm" aria-label="Cases">
            <thead className="border-b border-border bg-surface-2">
              <tr>
                <th className="w-10 px-3 py-2">
                  <span className="sr-only">Tick</span>
                </th>
                <th className={th}>Case</th>
                <th className={th}>Title</th>
                <th className={th}>State</th>
                <th className={th}>Exported</th>
                <th className={th}>Where</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.case_id} className="border-b border-border last:border-b-0 hover:bg-surface-2/60">
                  <td className="px-3 py-2 align-middle">
                    <Checkbox
                      checked={ticked.has(r.case_id)}
                      disabled={!tickable(r) || busy}
                      ariaLabel={`Tick #${r.case_id} ${r.title}`}
                      onCheckedChange={(on) => tick(r.case_id, on)}
                    />
                  </td>
                  <td className="whitespace-nowrap px-3 py-2 font-mono text-xs text-muted">{`#${r.case_id}`}</td>
                  <td className="px-3 py-2 text-text">{r.title}</td>
                  <td className="px-3 py-2">
                    <StatusBadge tone={stateTone(r.state)}>{stateText(r.state)}</StatusBadge>
                    {r.state.kind === "OutOfDate" && <div className="mt-0.5 text-xs text-muted">{r.state.why}</div>}
                  </td>
                  <td className="whitespace-nowrap px-3 py-2 text-xs text-muted">
                    {r.exported_at ? `exported ${localTime(r.exported_at)}` : <span className="text-faint">-</span>}
                  </td>
                  <td className="px-3 py-2 text-xs text-muted">
                    {(r.state.kind === "InRepoNotFromApp" || r.state.kind === "ExcludedInRepo") && r.repo_note}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div ref={resultRef}>
        {nothingNew && <Hint>Nothing new to add</Hint>}
        {result && !nothingNew && (
          <Card title="Updated" aria-label="Updated" bodyClassName="space-y-2 p-3">
            {result.cases.length > 0 && (
              <ul className="space-y-0.5 text-xs text-text">
                {result.cases.map(([id, file]) => (
                  <li key={id}>{`#${id} → ${file}: ${NEEDS_REPO}`}</li>
                ))}
              </ul>
            )}
            {(result.skipped ?? []).length > 0 && (
              <div className="space-y-0.5" aria-label="Skipped">
                <p className="text-xs font-medium text-warning">Not added, and why:</p>
                <ul className="space-y-0.5 text-xs text-warning">
                  {(result.skipped ?? []).map(([id, why]) => (
                    <li key={id}>{`#${id}: ${why}`}</li>
                  ))}
                </ul>
              </div>
            )}
            {result.missing_navigation.length > 0 && (
              <p className="text-xs text-warning">{`The clone's navigation.json has no entry for: ${result.missing_navigation.join(", ")}`}</p>
            )}
            {result.record_error && <p className="text-xs text-warning">{result.record_error}</p>}
            <FinishInRepo clone={clone} org={org} project={project} result={result} />
          </Card>
        )}
      </div>
    </section>
  );
}
