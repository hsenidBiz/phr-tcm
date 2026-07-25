// Pipeline history for one pull request, as a timeline of runs.
//
// The PR row shows only the latest run; this is where you dig. Each run
// expands into its stages, jobs and steps - the level ADO pins a failure
// to - with a search box and a failures-only filter for the case this
// exists to serve: "it went red, where?".

import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertCircle,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Rocket,
  Search,
  X,
} from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import type { BuildStage, PrBuild } from "../bindings";
import { cn } from "../lib/cn";
import { Input } from "./ui/input";
import { Modal } from "./ui/modal";

/** Tone for a pipeline/environment outcome. Anything still moving reads as
 * accent, so "in flight" is distinct from both pass and fail. */
export function tone(state: string, result?: string) {
  const s = result && result !== "" ? result : state;
  if (s === "succeeded") return "bg-success/15 text-success";
  if (s === "partiallySucceeded") return "bg-warning/15 text-warning";
  if (s === "failed" || s === "rejected") return "bg-danger/15 text-danger";
  if (s === "inProgress" || s === "queued" || s === "scheduled") return "bg-accent-soft text-accent";
  return "bg-surface-2 text-muted"; // notStarted, canceled, skipped, pending
}

const STATE_LABEL: Record<string, string> = {
  succeeded: "succeeded",
  partiallySucceeded: "partly succeeded",
  failed: "failed",
  rejected: "rejected",
  canceled: "canceled",
  inProgress: "in progress",
  notStarted: "not started",
  queued: "queued",
  scheduled: "scheduled",
  skipped: "skipped",
  abandoned: "abandoned",
  pending: "pending",
};
export const label = (s: string) => STATE_LABEL[s] ?? s;

const isFailure = (result: string) => result === "failed" || result === "rejected";
const isRunning = (state: string) => state === "inProgress";

export function duration(from: string, to: string) {
  if (!from) return "";
  const end = to ? new Date(to).getTime() : Date.now();
  const ms = end - new Date(from).getTime();
  if (!Number.isFinite(ms) || ms <= 0) return "";
  const mins = Math.floor(ms / 60000);
  const secs = Math.round((ms % 60000) / 1000);
  return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
}

// Named openExternal, not `open`: RunNode has an `open` state that would
// shadow it and silently turn these calls into a boolean invocation.
const openExternal = (url: string) => {
  if (!url) return;
  openUrl(url).catch(() => toast.error("Could not open the browser."));
};

/** The furthest environment a set of runs actually reached. */
export function furthest(builds: PrBuild[]): string {
  const deployed = builds.flatMap((b) => b.deployments).filter((d) => d.status === "succeeded");
  if (deployed.length > 0) return `Reached ${deployed[deployed.length - 1].environment}`;
  if (builds.some((b) => isFailure(b.result))) return "Failed before deploying";
  if (builds.some((b) => b.status !== "completed")) return "Building";
  return "Not deployed yet";
}

/** Where a run went wrong, as "Stage › Job › Step". Empty when it didn't. */
export function failurePath(b: PrBuild): string {
  for (const st of b.stages) {
    for (const j of st.jobs) {
      const task = j.tasks.find((t) => isFailure(t.result));
      if (task) return `${st.name} › ${j.name} › ${task.name}`;
    }
    if (isFailure(st.result) && st.jobs.every((j) => !j.tasks.some((t) => isFailure(t.result)))) {
      const job = st.jobs.find((j) => isFailure(j.result));
      return job ? `${st.name} › ${job.name}` : st.name;
    }
  }
  return "";
}

/** What is running right now, as "Stage › Job › Step". */
function runningPath(b: PrBuild): string {
  for (const st of b.stages) {
    if (!isRunning(st.state) && st.state !== "pending") continue;
    for (const j of st.jobs) {
      const task = j.tasks.find((t) => isRunning(t.state));
      if (task) return `${st.name} › ${j.name} › ${task.name}`;
      if (isRunning(j.state)) return `${st.name} › ${j.name}`;
    }
    if (isRunning(st.state)) return st.name;
  }
  return "";
}

function Dot({ state, result }: { state: string; result: string }) {
  const s = result || state;
  return (
    <span
      className={cn(
        "inline-block h-2 w-2 shrink-0 rounded-full",
        s === "succeeded"
          ? "bg-success"
          : isFailure(s)
            ? "bg-danger"
            : s === "inProgress"
              ? "animate-pulse bg-accent"
              : "bg-border-strong",
      )}
    />
  );
}

/** Stage -> job -> step tree for one run, filtered by the search box. */
function StageTree({
  stages,
  query,
  failuresOnly,
}: {
  stages: BuildStage[];
  query: string;
  failuresOnly: boolean;
}) {
  const q = query.trim().toLowerCase();
  const hit = (name: string) => !q || name.toLowerCase().includes(q);

  const filtered = stages
    .map((st) => {
      const jobs = st.jobs
        .map((j) => {
          const tasks = j.tasks.filter(
            (t) =>
              (!failuresOnly || isFailure(t.result)) &&
              (hit(t.name) || hit(j.name) || hit(st.name)),
          );
          const jobMatches = (!failuresOnly || isFailure(j.result)) && hit(j.name);
          return tasks.length > 0 || jobMatches ? { ...j, tasks } : null;
        })
        .filter((j): j is BuildStage["jobs"][number] => j !== null);
      const stageMatches = (!failuresOnly || isFailure(st.result)) && hit(st.name);
      return jobs.length > 0 || stageMatches ? { ...st, jobs } : null;
    })
    .filter((st): st is BuildStage => st !== null);

  if (filtered.length === 0) {
    return (
      <p className="py-1 text-xs text-faint">
        {failuresOnly ? "No failures in this run." : "Nothing matches that search."}
      </p>
    );
  }

  return (
    <ul className="space-y-2">
      {filtered.map((st, si) => (
        <li key={`${st.name}-${si}`} className="space-y-1">
          <div className="flex items-center gap-2 text-xs">
            <Dot state={st.state} result={st.result} />
            <span className="font-medium text-text">{st.name}</span>
            <span className="text-faint">{label(st.result || st.state)}</span>
            {duration(st.started, st.finished) && (
              <span className="text-faint">· {duration(st.started, st.finished)}</span>
            )}
          </div>
          {st.jobs.map((j, ji) => (
            <div key={`${j.name}-${ji}`} className="ml-3 space-y-1 border-l border-border pl-3">
              <div className="flex items-center gap-2 text-xs">
                <Dot state={j.state} result={j.result} />
                <span className="text-text">{j.name}</span>
                <span className="text-faint">{label(j.result || j.state)}</span>
              </div>
              {j.tasks.length > 0 && (
                <ul className="ml-3 space-y-0.5 border-l border-border pl-3">
                  {j.tasks.map((t, ti) => (
                    <li key={`${t.name}-${ti}`} className="space-y-0.5">
                      <div className="flex items-center gap-2 text-xs">
                        <Dot state={t.state} result={t.result} />
                        <span
                          className={cn(
                            "truncate",
                            isFailure(t.result) ? "font-medium text-danger" : "text-muted",
                          )}
                        >
                          {t.name}
                        </span>
                        {duration(t.started, t.finished) && (
                          <span className="ml-auto shrink-0 text-faint">
                            {duration(t.started, t.finished)}
                          </span>
                        )}
                      </div>
                      {/* The actual reason, without opening Azure DevOps. */}
                      {t.issues.map((msg, mi) => (
                        <p
                          key={mi}
                          className="ml-4 flex items-start gap-1 rounded bg-danger/10 px-1.5 py-1 text-[11px] text-danger"
                        >
                          <AlertCircle size={11} className="mt-0.5 shrink-0" />
                          <span className="break-words">{msg}</span>
                        </p>
                      ))}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ))}
        </li>
      ))}
    </ul>
  );
}

function RunNode({
  b,
  defaultOpen,
  query,
  failuresOnly,
}: {
  b: PrBuild;
  defaultOpen: boolean;
  query: string;
  failuresOnly: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const failed = failurePath(b);
  const running = runningPath(b);
  // A search should reveal what it matched, not hide it behind a collapse.
  const expanded = open || query.trim() !== "" || failuresOnly;

  return (
    <li className="relative pl-6">
      {/* Timeline rail + node. */}
      <span className="absolute left-0 top-2 flex h-4 w-4 items-center justify-center">
        <Dot state={b.status} result={b.result} />
      </span>
      <div className="space-y-2 rounded-md border border-border bg-bg p-3">
        <button
          className="flex w-full items-center gap-2 text-left"
          aria-expanded={expanded}
          onClick={() => setOpen((o) => !o)}
        >
          {expanded ? (
            <ChevronDown size={13} className="shrink-0 text-muted" />
          ) : (
            <ChevronRight size={13} className="shrink-0 text-muted" />
          )}
          <span
            className={cn(
              "shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium",
              tone(b.status, b.result),
            )}
          >
            {label(b.result || b.status)}
          </span>
          <span className="truncate font-medium text-text">{b.name}</span>
          <span className="id-mono shrink-0 text-xs text-faint">{b.number}</span>
          <span className="shrink-0 rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-muted">
            {b.is_validation ? "PR validation" : "CI after merge"}
          </span>
        </button>

        <p className="text-xs text-faint">
          {b.started ? new Date(b.started).toLocaleString() : "Not started"}
          {duration(b.started, b.finished) && ` · ${duration(b.started, b.finished)}`}
        </p>

        {/* Lead with the answer: where it broke, or what is running now. */}
        {failed && (
          <p className="flex items-start gap-1 rounded bg-danger/10 px-2 py-1 text-xs text-danger">
            <AlertCircle size={12} className="mt-0.5 shrink-0" />
            <span>Failed at {failed}</span>
          </p>
        )}
        {!failed && running && (
          <p className="rounded bg-accent-soft px-2 py-1 text-xs text-accent">Running {running}</p>
        )}

        {expanded && (
          <>
            {b.stages.length > 0 ? (
              <StageTree stages={b.stages} query={query} failuresOnly={failuresOnly} />
            ) : (
              <p className="text-xs text-faint">No stage detail available for this run.</p>
            )}

            {b.deployments.length > 0 && (
              <div className="space-y-1 border-t border-border/60 pt-2">
                <p className="flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide text-faint">
                  <Rocket size={11} /> Environments
                </p>
                <ul className="space-y-0.5">
                  {b.deployments.map((d, i) => (
                    <li key={i} className="flex items-center gap-2 text-xs">
                      <span
                        className={cn(
                          "rounded-full px-2 py-0.5 text-[11px] font-medium",
                          tone(d.status),
                        )}
                      >
                        {d.environment}
                      </span>
                      <span className="text-faint">{label(d.status)}</span>
                      {d.on && (
                        <span className="text-faint">· {new Date(d.on).toLocaleString()}</span>
                      )}
                      {d.web_url && (
                        <button
                          aria-label={`Open ${d.release} in Azure DevOps`}
                          title={`Open ${d.release} in Azure DevOps`}
                          className="ml-auto rounded p-0.5 text-muted hover:text-accent"
                          onClick={() => openExternal(d.web_url)}
                        >
                          <ExternalLink size={11} />
                        </button>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {b.web_url && (
              <button
                className="text-xs text-muted underline-offset-2 hover:text-accent hover:underline"
                onClick={() => openExternal(b.web_url)}
              >
                Open run in Azure DevOps
              </button>
            )}
          </>
        )}
      </div>
    </li>
  );
}

export default function PipelineDialog({
  prId,
  prTitle,
  repo,
  builds,
  onClose,
}: {
  prId: number;
  prTitle: string;
  repo: string;
  builds: PrBuild[];
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [failuresOnly, setFailuresOnly] = useState(false);

  // Open the run that needs attention: the newest failing or running one,
  // else the newest. Anything else starts collapsed so the timeline reads.
  const focusId = useMemo(() => {
    const notable = builds.find((b) => isFailure(b.result) || b.status !== "completed");
    return (notable ?? builds[0])?.id;
  }, [builds]);

  const anyFailures = builds.some((b) => failurePath(b) !== "");

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-2xl flex-col">
      <header className="flex items-start gap-3 border-b border-border px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold text-text">
            <span className="id-mono text-faint">!{prId}</span> {prTitle}
          </h2>
          <p className="mt-0.5 text-xs text-muted">
            {repo} · {builds.length} run{builds.length === 1 ? "" : "s"} · {furthest(builds)}
          </p>
        </div>
        <button
          aria-label="Close pipeline history"
          className="rounded p-1 text-muted hover:text-text"
          onClick={onClose}
        >
          <X size={16} />
        </button>
      </header>

      {builds.length > 0 && (
        <div className="flex items-center gap-2 border-b border-border px-4 py-2">
          <div className="relative flex-1">
            <Search
              size={13}
              className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-faint"
            />
            <Input
              aria-label="Search stages and steps"
              placeholder="Search stages, jobs and steps…"
              className="py-1 pl-7 text-xs"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <button
            aria-pressed={failuresOnly}
            disabled={!anyFailures}
            title={anyFailures ? "Show only failed steps" : "Nothing failed in these runs"}
            className={cn(
              "rounded border px-2 py-1 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40",
              failuresOnly
                ? "border-danger/40 bg-danger/15 text-danger"
                : "border-border text-muted hover:text-text",
            )}
            onClick={() => setFailuresOnly((f) => !f)}
          >
            Failures only
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {builds.length === 0 ? (
          <p className="text-sm text-faint">No builds found for this pull request.</p>
        ) : (
          // The rail: a single line the run nodes sit on, newest first.
          <ol className="relative space-y-3 before:absolute before:bottom-2 before:left-2 before:top-2 before:w-px before:bg-border">
            {builds.map((b) => (
              <RunNode
                key={b.id}
                b={b}
                defaultOpen={b.id === focusId}
                query={query}
                failuresOnly={failuresOnly}
              />
            ))}
          </ol>
        )}
      </div>
    </Modal>
  );
}
