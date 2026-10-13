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
import { useEffect, useState } from "react";
import { commands, events, type ExportResult, type SyncRow, type SyncState } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { IconAdd, IconExport, IconRefresh } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import FinishInRepo from "./FinishInRepo";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query key of the states; the clone, org and project are part of it. */
export const SYNC_KEY = "pw-sync";

/** A UTC stamp as a local date and time; anything unreadable as it is. */
function localTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** What each updated case still needs in the repo: said here only. */
export const NEEDS_REPO = "needs /gen-test steps 5, 6 and 7 in the repo (Finish in the repo, below)";

function stateText(s: SyncState): string {
  switch (s.kind) {
    case "Current":
      return "Current";
    case "OutOfDate":
      return `Out of date: ${s.why}`;
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

function stateTone(s: SyncState): string {
  switch (s.kind) {
    case "Current":
      return "text-success";
    case "OutOfDate":
    case "NotInRepo":
      return "text-warning";
    case "NeverExported":
    case "InRepoNotFromApp":
    case "ExcludedInRepo":
      return "text-muted";
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
  });
  const [ticked, setTicked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ExportResult | null>(null);
  /** The last write was Add new: an empty one says there was nothing new. */
  const [wasAddNew, setWasAddNew] = useState(false);

  // A script saved, repaired or removed anywhere: its state may have changed.
  useEffect(() => {
    const un = events.autorunScriptsChanged.listen(() => {
      void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
    });
    return () => {
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

  return (
    <section className="space-y-2" aria-label="Sync">
      <h3 className={sectionTitle}>Sync</h3>
      <p className="text-xs text-muted">
        Whether the repo's copy of each exported script is current. Update exports the cases again: each raw spec is
        replaced and each case's section is rewritten from Azure DevOps; the repo's Added by engineer, Also and
        Assertion floor blocks and every other section and file stay as they are.
      </p>
      <p className="text-xs text-muted">
        Add new exports every case the repo does not have yet. A case the repo has that this app did not export is
        left alone unless you tick it and use Update ticked.
      </p>
      <div className="flex flex-wrap items-center gap-2">
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
        <Button size="sm" variant="outline" disabled={busy || states.isFetching} onClick={() => void states.refetch()}>
          <IconRefresh aria-hidden />
          Refresh
        </Button>
      </div>
      {busy && <p className="text-xs text-muted">Updating…</p>}
      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}

      {nothingNew && <p className="text-xs text-muted">Nothing new to add</p>}
      {result && !nothingNew && (
        <div className="space-y-1" aria-label="Updated">
          {result.cases.length > 0 && <p className="text-xs text-text">Updated:</p>}
          <ul className="space-y-0.5 text-xs text-text">
            {result.cases.map(([id, file]) => (
              <li key={id}>{`#${id} → ${file}: ${NEEDS_REPO}`}</li>
            ))}
          </ul>
          {(result.skipped ?? []).length > 0 && (
            <div className="space-y-0.5" aria-label="Skipped">
              <p className="text-xs text-warning">Not added, and why:</p>
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
        </div>
      )}

      {states.isLoading && <p className="text-xs text-muted">Reading what was exported…</p>}
      {states.isError && <p className="whitespace-pre-wrap text-xs text-danger">{states.error.message}</p>}
      {states.isSuccess && rows.length === 0 && <p className="text-xs text-muted">No scripts in this project yet.</p>}
      <ul className="space-y-1">
        {rows.map((r) => (
          <li key={r.case_id} className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
            <Checkbox
              checked={ticked.has(r.case_id)}
              disabled={!tickable(r) || busy}
              ariaLabel={`Tick #${r.case_id} ${r.title}`}
              onCheckedChange={(on) => tick(r.case_id, on)}
            />
            <span className="text-text">{`#${r.case_id} ${r.title}`}</span>
            <span className={`text-xs ${stateTone(r.state)}`}>{stateText(r.state)}</span>
            {r.exported_at && <span className="text-xs text-muted">{`exported ${localTime(r.exported_at)}`}</span>}
            {(r.state.kind === "InRepoNotFromApp" || r.state.kind === "ExcludedInRepo") && r.repo_note && (
              <span className="text-xs text-muted">{r.repo_note}</span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
