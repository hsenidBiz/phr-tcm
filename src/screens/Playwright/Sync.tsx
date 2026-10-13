// The Playwright tab's Sync: whether the clone's copy of each exported Auto
// Run script is current, and Update, which exports the chosen cases again
// through Export to Playwright's own all-or-nothing write (pw_repo::sync).
//
// Each case is one of: current, out of date (its script changed after the
// export, and what changed), not in the repo (its section or raw spec is
// gone), or never exported. A script change anywhere in the app
// (`autorunScriptsChanged`) reads the states again.
//
// Update replaces a case's raw spec and its section in the test-case file;
// the repo's own blocks, other sections and files are left as they are. A
// raw spec still needs the repo's refactorer, which each updated case says.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands, events, type ExportResult, type SyncRow, type SyncState } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { IconExport, IconRefresh } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query key of the states; the clone, org and project are part of it. */
export const SYNC_KEY = "pw-sync";

/** What each updated case still needs in the repo. */
export const NEEDS_REPO = "needs /gen-test stages 5 and 6 in the repo";

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
  const picked = rows.filter((r) => r.has_script && ticked.has(r.case_id)).map((r) => r.case_id);

  const update = async (cases: number[]) => {
    if (cases.length === 0) return;
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const r = await unwrapStr(
        commands.pwSyncUpdate(clone, org, project, cases, prefs.moduleRef ?? null, prefs.preconditionsRef ?? null),
      );
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
        Whether the repo's copy of each exported script is current. Update exports the cases again: their raw specs
        and their sections are replaced, and nothing else in the repo is changed.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy || outOfDate.length === 0} onClick={() => void update(outOfDate)}>
          <IconExport aria-hidden />
          {`Update out of date (${outOfDate.length})`}
        </Button>
        <Button size="sm" variant="outline" disabled={busy || picked.length === 0} onClick={() => void update(picked)}>
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

      {result && (
        <div className="space-y-1" aria-label="Updated">
          <p className="text-xs text-text">Updated:</p>
          <ul className="space-y-0.5 text-xs text-text">
            {result.cases.map(([id, file]) => (
              <li key={id}>{`#${id} → ${file}: ${NEEDS_REPO}`}</li>
            ))}
          </ul>
          {result.missing_navigation.length > 0 && (
            <p className="text-xs text-warning">{`The clone's navigation.json has no entry for: ${result.missing_navigation.join(", ")}`}</p>
          )}
          {result.record_error && <p className="text-xs text-warning">{result.record_error}</p>}
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
              disabled={!r.has_script || busy}
              ariaLabel={`Tick #${r.case_id} ${r.title}`}
              onCheckedChange={(on) => tick(r.case_id, on)}
            />
            <span className="text-text">{`#${r.case_id} ${r.title}`}</span>
            <span className={`text-xs ${stateTone(r.state)}`}>{stateText(r.state)}</span>
            {r.exported_at && <span className="text-xs text-muted">{`exported ${r.exported_at}`}</span>}
          </li>
        ))}
      </ul>
    </section>
  );
}
