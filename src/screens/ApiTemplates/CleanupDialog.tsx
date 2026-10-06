import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { commands, events, type CleanupLine } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconRemove, IconStop } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import { logUi } from "../../lib/uiLog";

/** The preview query's key prefix. */
export const CLEANUP_KEY = "autorun-cleanup-preview";

/** Said when the preview has nothing in it. */
export const NOTHING_MATCHES = "No test-made drafts match.";

/** Said when a cleanup failed in a way the command gave no sentence for. */
export const COULD_NOT_CLEAN_UP = "Could not clean up. Try again, or see Settings → Logs.";

/** The confirm sentence, exactly as the design gives it. */
export function confirmSentence(n: number, environment: string): string {
  return `Delete ${n} drafts from ${environment}? This cannot be undone.`;
}

/** How old a draft is, in whole days. */
function age(createdAt: string): string {
  const ms = Date.now() - Date.parse(createdAt);
  if (!Number.isFinite(ms)) return "age unknown";
  const days = Math.max(0, Math.floor(ms / 86_400_000));
  return days === 1 ? "1 day old" : `${days} days old`;
}

/** One result as it came in. */
type Result = { id: string; outcome: string };

type Phase = "choose" | "confirm" | "running" | "done";

/**
 * Clean up test-made drafts: the person picks the environment, the name
 * prefix and the age, looks at what the record of test-made drafts holds
 * that matches, and deletes the ticked ones through their proven delete
 * templates, one at a time, watching each result come in.
 *
 * Only what Rust previews can be deleted, and Rust checks the preview again
 * when the deletes start: the webview only ever sends the ids ticked. A
 * line with no proven delete template for its kind cannot be ticked.
 */
export default function CleanupDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  const headingId = useId();
  const envs = useEnvironments();
  const [envId, setEnvId] = useState<string | null>(null);
  const [prefix, setPrefix] = useState<string | null>(null);
  const [olderThan, setOlderThan] = useState("7");
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [phase, setPhase] = useState<Phase>("choose");
  const [results, setResults] = useState<Result[]>([]);
  const [failed, setFailed] = useState<string | null>(null);
  const [stopping, setStopping] = useState(false);
  const listening = useRef(false);

  const list = envs.data?.environments ?? [];
  const chosen = list.find((e) => e.id === envId) ?? list.find((e) => e.id === envs.data?.active);
  const chosenPrefix = prefix ?? chosen?.test_prefix ?? "";
  const days = Number(olderThan);
  const isActive = Boolean(chosen && chosen.id === envs.data?.active);

  const preview = useQuery({
    queryKey: [CLEANUP_KEY, org, project, chosen?.id, chosenPrefix, olderThan],
    queryFn: () =>
      unwrapStr(
        commands.autoRunCleanupPreview(org, project, chosen!.id, chosenPrefix, Number.isInteger(days) ? days : 0),
      ),
    enabled: Boolean(org && project && chosen),
    retry: false,
  });
  const lines: CleanupLine[] = preview.data ?? [];

  // Every deletable line starts ticked, each time the preview changes.
  useEffect(() => {
    if (preview.data) setTicked(new Set(preview.data.filter((l) => l.deletable).map((l) => l.entry.id)));
  }, [preview.data]);

  // Each delete's result, as it comes.
  useEffect(() => {
    const un = events.autorunCleanupProgress.listen((e) => {
      if (!listening.current) return;
      setResults((prev) => [...prev, { id: e.payload.id, outcome: e.payload.outcome }]);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const chosenIds = lines.filter((l) => l.deletable && ticked.has(l.entry.id)).map((l) => l.entry.id);

  const toggle = (id: string, on: boolean) =>
    setTicked((prev) => {
      const next = new Set(prev);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const start = async () => {
    if (!chosen) return;
    setResults([]);
    setFailed(null);
    setStopping(false);
    setPhase("running");
    listening.current = true;
    try {
      const r = await commands.autoRunCleanupRun(org, project, chosen.id, chosenIds);
      if (r.status === "error") setFailed(r.error);
    } catch (e) {
      logUi(`clean up: ${e instanceof Error ? e.message : String(e)}`);
      setFailed(COULD_NOT_CLEAN_UP);
    }
    listening.current = false;
    setPhase("done");
    await preview.refetch();
  };

  const stop = async () => {
    setStopping(true);
    try {
      const r = await commands.autoRunCleanupStop();
      if (r.status === "error") setFailed(r.error);
    } catch (e) {
      logUi(`clean up stop: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const busy = phase === "running";
  const lineOf = (id: string) => lines.find((l) => l.entry.id === id);

  return (
    <Modal
      onClose={busy ? () => undefined : onClose}
      labelledBy={headingId}
      className="flex max-h-[85vh] w-full max-w-2xl flex-col gap-3 p-5"
    >
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Clean up test-made drafts
      </h2>
      <p className="text-xs text-muted">
        Only drafts the tests made are listed: what fixtures and setups recorded as they made it. Each is
        deleted by the proven delete template for its kind.
      </p>

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-xs text-muted">
          Environment
          <Select
            aria-label="Environment"
            value={chosen?.id ?? ""}
            disabled={busy}
            onChange={(e) => {
              setEnvId(e.target.value);
              setPrefix(null);
              setPhase("choose");
            }}
          >
            {list.map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted">
          Name starts with
          <Input
            aria-label="Name starts with"
            value={chosenPrefix}
            disabled={busy}
            onChange={(e) => {
              setPrefix(e.target.value);
              setPhase("choose");
            }}
            className="w-40"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted">
          Older than (days)
          <Input
            aria-label="Older than (days)"
            type="number"
            min={1}
            step={1}
            value={olderThan}
            disabled={busy}
            onChange={(e) => {
              setOlderThan(e.target.value);
              setPhase("choose");
            }}
            className="w-24"
          />
        </label>
      </div>

      {chosen && !isActive && (
        <p className="text-xs text-warning">
          {`Clean up only deletes in the active environment. Switch to ${chosen.name} first.`}
        </p>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {envs.isError && <p className="text-sm text-danger">{envs.error.message}</p>}
        {preview.isLoading && <p className="text-sm text-muted">Looking for drafts…</p>}
        {preview.isError && <p className="text-sm text-danger">{preview.error.message}</p>}
        {preview.data && lines.length === 0 && <p className="text-sm text-muted">{NOTHING_MATCHES}</p>}
        {lines.length > 0 && (
          <ul aria-label="Drafts to clean up" className="space-y-1">
            {lines.map((l) => {
              const e = l.entry;
              return (
                <li
                  key={`${e.kind}:${e.id}`}
                  aria-label={`${e.kind} ${e.name}`}
                  className="flex items-start gap-2 rounded-md border border-border bg-surface px-3 py-2"
                >
                  <Checkbox
                    ariaLabel={`Delete ${e.kind} ${e.name}`}
                    checked={l.deletable && ticked.has(e.id)}
                    disabled={!l.deletable || busy}
                    onCheckedChange={(on) => toggle(e.id, on)}
                    className="mt-0.5"
                  />
                  <div className="min-w-0 flex-1 space-y-0.5">
                    <div className="flex flex-wrap items-baseline gap-x-2 text-sm">
                      <span className="text-xs text-muted">{e.kind}</span>
                      <span className="break-words font-medium text-text">{e.name || "(no name)"}</span>
                      <span className="text-xs text-muted">{`id ${e.id}`}</span>
                    </div>
                    <div className="flex flex-wrap gap-x-3 text-xs text-muted">
                      <span>{age(e.created_at)}</span>
                      <span>{`made by ${e.fixture}`}</span>
                      {e.status !== "present" && <span className="text-warning">{e.status}</span>}
                    </div>
                    {l.note && <p className="text-xs text-faint">{l.note}</p>}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {(results.length > 0 || failed) && (
        <div className="space-y-1">
          {results.length > 0 && (
            <ul aria-label="Results" className="space-y-0.5 text-xs">
              {results.map((r, i) => {
                const line = lineOf(r.id);
                const ok = r.outcome === "deleted";
                return (
                  <li key={`${r.id}-${i}`} className={cn(ok ? "text-success" : "text-danger")}>
                    {`${line ? `${line.entry.kind} ${line.entry.name}` : r.id}: ${ok ? "deleted" : r.outcome}`}
                  </li>
                );
              })}
            </ul>
          )}
          {failed && <p className="text-xs text-danger">{failed}</p>}
        </div>
      )}

      {phase === "confirm" && chosen ? (
        <div className="space-y-2 rounded-md border border-border bg-surface px-3 py-2">
          <p className="text-sm text-text">{confirmSentence(chosenIds.length, chosen.name)}</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPhase("choose")}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" variant="danger" onClick={() => void start()}>
              <IconRemove aria-hidden />
              Delete
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex justify-end gap-2">
          {busy ? (
            <Button size="sm" variant="outline" disabled={stopping} onClick={() => void stop()}>
              <IconStop aria-hidden />
              {stopping ? "Stopping…" : "Stop"}
            </Button>
          ) : (
            <>
              <Button size="sm" variant="ghost" onClick={onClose}>
                <IconCancel aria-hidden />
                Close
              </Button>
              <Button
                size="sm"
                variant="danger"
                disabled={chosenIds.length === 0 || !isActive}
                onClick={() => setPhase("confirm")}
              >
                <IconRemove aria-hidden />
                {`Delete ${chosenIds.length} drafts`}
              </Button>
            </>
          )}
        </div>
      )}
    </Modal>
  );
}
