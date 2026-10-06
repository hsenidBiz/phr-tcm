import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { commands, type FixtureRun, type SavedFixture } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconRefresh, IconRemove, IconRun } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { relativeTime } from "../../lib/history";
import { unwrapStr } from "../../lib/ipc";
import { logUi } from "../../lib/uiLog";
import RemoveFixture from "./RemoveFixture";
import { stampDate } from "./TemplateRow";

/** The fixtures query's key prefix, invalidated with the templates'. */
export const FIXTURES_KEY = "api-fixtures";

/** A captured value as the row shows it: text as it is, anything else as JSON. */
function valueText(v: unknown): string {
  return typeof v === "string" ? v : JSON.stringify(v);
}

/** The outputs of the newest run that passed - what scripts using the
 * fixture read now. Runs come newest first. */
function currentOutputs(runs: FixtureRun[]): [string, string][] {
  const built = runs.find((r) => r.ok);
  return Object.entries(built?.outputs ?? {}).map(([k, v]) => [k, valueText(v)]);
}

function LastRun({ run }: { run: FixtureRun | undefined }) {
  if (!run) return <span className="text-xs text-faint">never run</span>;
  const d = stampDate(run.at);
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted">
      <span aria-hidden className={cn("size-2 rounded-full", run.ok ? "bg-success" : "bg-danger")} />
      last run {d ? relativeTime(d.toISOString()) : run.at}, {run.ok ? "succeeded" : "failed"}
    </span>
  );
}

/** Said when a run fails in a way the command gave no sentence for. */
export const COULD_NOT_RUN = "Could not run the fixture. Try again, or see Settings → Logs.";

/** A raw rejection (the call itself failed, not a refusal of the command)
 * is logged and never shown. */
function rawFailure(e: unknown): string {
  logUi(`fixture run: ${e instanceof Error ? e.message : String(e)}`);
  return COULD_NOT_RUN;
}

/** What the last press of Run or Rebuild said, kept in the row until the
 * next one: the failure sentence, and any warnings from a run. */
type Said = { failed: string | null; warnings: string[] };

/**
 * The Fixtures tab: the drafts an assistant has taught the app to build,
 * each with its steps, what it last made and when it last ran. A person runs
 * one here (Run the first time, Rebuild after), and can remove it.
 *
 * Only one template or fixture runs at a time, so while a run is going its
 * button is busy and every other fixture's is off. A run that fails leaves
 * the previous outputs on show: they are what scripts still read.
 */
export default function FixturesTab({
  org,
  project,
  cleanupSlot,
}: {
  org: string;
  project: string;
  /** Where Clean up test-made drafts sits, above the list. */
  cleanupSlot?: ReactNode;
}) {
  const qc = useQueryClient();
  const fixtures = useQuery({
    queryKey: [FIXTURES_KEY, org, project],
    queryFn: () => unwrapStr(commands.apiFixturesList(org, project)),
    enabled: Boolean(org && project),
  });
  const [runningId, setRunningId] = useState<string | null>(null);
  const [said, setSaid] = useState<Record<string, Said>>({});
  const [removing, setRemoving] = useState<SavedFixture["fixture"] | null>(null);

  const run = async (id: string) => {
    setRunningId(id);
    let result: Said;
    try {
      const r = await commands.apiFixtureRun(org, project, id);
      if (r.status === "error") {
        // The command's own sentence.
        result = { failed: r.error, warnings: [] };
      } else {
        result = {
          failed: r.data.ok ? null : (r.data.failed ?? "The fixture did not finish."),
          warnings: r.data.warnings,
        };
      }
    } catch (e) {
      result = { failed: rawFailure(e), warnings: [] };
    }
    setSaid((prev) => ({ ...prev, [id]: result }));
    setRunningId(null);
    await qc.invalidateQueries({ queryKey: [FIXTURES_KEY] });
  };

  const list = fixtures.data ?? [];

  return (
    <div className="space-y-2">
      {cleanupSlot && <div className="flex justify-end">{cleanupSlot}</div>}
      {fixtures.isLoading && <p className="text-sm text-muted">Loading fixtures…</p>}
      {fixtures.isError && <p className="text-sm text-danger">{fixtures.error.message}</p>}
      {fixtures.data && list.length === 0 && (
        <p className="text-sm text-muted">
          No fixtures yet. Your assistant saves one when a script needs a draft of its own: the templates
          to run, in order, and what they make.
        </p>
      )}
      {list.length > 0 && (
        <ul className="space-y-1">
          {list.map(({ fixture: f, runs }) => {
            const outputs = currentOutputs(runs);
            const built = runs.some((r) => r.ok);
            const busy = runningId === f.id;
            const result = said[f.id];
            return (
              <li
                key={f.id}
                aria-label={f.name}
                className="space-y-1.5 rounded-md border border-border bg-surface px-3 py-2"
              >
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="min-w-0 flex-1 break-words text-sm font-medium text-text">{f.name}</span>
                  <span className="text-xs text-muted">{`Runs as ${f.account}`}</span>
                  <LastRun run={runs[0]} />
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={runningId !== null}
                    aria-label={`${built ? "Rebuild" : "Run"} ${f.name}`}
                    onClick={() => void run(f.id)}
                  >
                    {built ? <IconRefresh aria-hidden /> : <IconRun aria-hidden />}
                    {busy ? "Running…" : built ? "Rebuild" : "Run"}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={runningId !== null}
                    aria-label={`Remove ${f.name}`}
                    onClick={() => setRemoving(f)}
                  >
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                </div>
                <p className="text-xs text-muted">
                  <span className="text-faint">Steps </span>
                  {f.steps.map((s) => s.template).join(", ")}
                </p>
                <p className="id-mono break-all text-xs text-muted">
                  <span className="font-sans text-faint">Outputs </span>
                  {outputs.length > 0 ? outputs.map(([k, v]) => `${k} = ${v}`).join(", ") : "none yet"}
                </p>
                {result?.failed && (
                  <p role="alert" className="text-xs text-danger">
                    {result.failed}
                  </p>
                )}
                {result?.warnings.map((w, i) => (
                  <p key={`${i}-${w}`} className="text-xs text-warning">
                    {w}
                  </p>
                ))}
              </li>
            );
          })}
        </ul>
      )}

      {removing && (
        <RemoveFixture
          org={org}
          project={project}
          fixture={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => void qc.invalidateQueries({ queryKey: [FIXTURES_KEY] })}
        />
      )}
    </div>
  );
}
