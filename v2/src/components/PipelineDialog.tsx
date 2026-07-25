// A readable pipeline history for one pull request: what ran, how it went,
// and - the question people actually open this for - how far the change
// travelled through the environments. The PR row keeps a compact summary;
// this is the roomy view behind it.

import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronRight, ExternalLink, Rocket, X } from "lucide-react";
import { toast } from "sonner";
import type { PrBuild } from "../bindings";
import { cn } from "../lib/cn";
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
  pending: "pending",
};
export const label = (s: string) => STATE_LABEL[s] ?? s;

function duration(from: string, to: string) {
  if (!from || !to) return "";
  const ms = new Date(to).getTime() - new Date(from).getTime();
  if (!Number.isFinite(ms) || ms <= 0) return "";
  const mins = Math.floor(ms / 60000);
  const secs = Math.round((ms % 60000) / 1000);
  return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
}

const open = (url: string) => {
  if (!url) return;
  openUrl(url).catch(() => toast.error("Could not open the browser."));
};

/** The headline: the furthest environment this PR actually reached. */
function furthest(builds: PrBuild[]): string {
  const deployed = builds
    .flatMap((b) => b.deployments)
    .filter((d) => d.status === "succeeded");
  if (deployed.length === 0) {
    const anyRunning = builds.some((b) => b.status !== "completed");
    return anyRunning ? "Building" : "Not deployed yet";
  }
  // Last succeeded environment in release order is the furthest reached.
  return `Reached ${deployed[deployed.length - 1].environment}`;
}

function Flow({
  items,
}: {
  items: { key: string; name: string; cls: string; title: string }[];
}) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      {items.map((it, i) => (
        <span key={it.key} className="flex items-center gap-1">
          {i > 0 && <ChevronRight size={12} className="shrink-0 text-faint" />}
          <span
            className={cn("rounded-full px-2 py-0.5 text-[11px] font-medium", it.cls)}
            title={it.title}
          >
            {it.name}
          </span>
        </span>
      ))}
    </div>
  );
}

function BuildBlock({ b }: { b: PrBuild }) {
  const took = duration(b.started, b.finished);
  return (
    <li className="space-y-2 rounded-md border border-border bg-bg p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={cn(
            "rounded-full px-2 py-0.5 text-[11px] font-medium",
            tone(b.status, b.result),
          )}
        >
          {label(b.result || b.status)}
        </span>
        <span className="font-medium text-text">{b.name}</span>
        <span className="id-mono text-xs text-faint">{b.number}</span>
        <span className="rounded bg-surface-2 px-1.5 py-0.5 text-[10px] text-muted">
          {b.is_validation ? "PR validation" : "CI after merge"}
        </span>
        {b.web_url && (
          <button
            aria-label={`Open build ${b.number} in Azure DevOps`}
            title="Open build in Azure DevOps"
            className="ml-auto rounded p-1 text-muted hover:text-accent"
            onClick={() => open(b.web_url)}
          >
            <ExternalLink size={13} />
          </button>
        )}
      </div>

      <p className="text-xs text-faint">
        {b.started ? new Date(b.started).toLocaleString() : "Not started"}
        {took && ` · ${took}`}
      </p>

      {b.stages.length > 0 && (
        <div className="space-y-1">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-faint">Stages</p>
          <Flow
            items={b.stages.map((s, i) => ({
              key: `${s.name}-${i}`,
              name: s.name,
              cls: tone(s.state, s.result),
              title: `${s.name}: ${label(s.result || s.state)}`,
            }))}
          />
        </div>
      )}

      {b.deployments.length > 0 && (
        <div className="space-y-1">
          <p className="flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide text-faint">
            <Rocket size={11} /> Environments
          </p>
          <Flow
            items={b.deployments.map((d, i) => ({
              key: `${d.environment}-${i}`,
              name: d.environment,
              cls: tone(d.status),
              title: `${d.release} → ${d.environment}: ${label(d.status)}${
                d.on ? ` (${new Date(d.on).toLocaleString()})` : ""
              }`,
            }))}
          />
          <ul className="space-y-0.5 pt-0.5">
            {b.deployments.map((d, i) => (
              <li key={i} className="flex items-center gap-2 text-xs">
                <span className="w-28 shrink-0 truncate text-muted">{d.environment}</span>
                <span className="text-faint">{label(d.status)}</span>
                {d.on && (
                  <span className="text-faint">· {new Date(d.on).toLocaleString()}</span>
                )}
                {d.web_url && (
                  <button
                    aria-label={`Open ${d.release} in Azure DevOps`}
                    title={`Open ${d.release} in Azure DevOps`}
                    className="ml-auto rounded p-0.5 text-muted hover:text-accent"
                    onClick={() => open(d.web_url)}
                  >
                    <ExternalLink size={11} />
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
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
  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-2xl flex-col">
      <header className="flex items-start gap-3 border-b border-border px-4 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold text-text">
            <span className="id-mono text-faint">!{prId}</span> {prTitle}
          </h2>
          <p className="mt-0.5 text-xs text-muted">
            {repo} · {builds.length} build{builds.length === 1 ? "" : "s"} · {furthest(builds)}
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

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {builds.length === 0 ? (
          <p className="text-sm text-faint">No builds found for this pull request.</p>
        ) : (
          <ol className="space-y-3">
            {builds.map((b) => (
              <BuildBlock key={b.id} b={b} />
            ))}
          </ol>
        )}
      </div>
    </Modal>
  );
}
