// The Work Manager's Pull Requests panel: three read-only groups ordered
// by actionability - awaiting your review, yours, then everything active
// on a chosen repo. Rows open the PR in the browser; voting/completing
// stays in Azure DevOps (this panel never writes).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Bug,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  GitBranch,
  GitPullRequest,
  RefreshCw,
  Rocket,
} from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type PrBuild, type PullRequest, type PrWorkItem } from "../bindings";
import PipelineDialog, { duration, failurePath, label, tone } from "../components/PipelineDialog";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import AstryxIsland from "../components/AstryxIsland";
import { Markdown } from "@astryxdesign/core/Markdown";

/** ADO reviewer votes: 10 approved, 5 approved w/ suggestions, 0 waiting,
 * -5 waiting for author, -10 rejected. */
function voteDot(vote: number): { cls: string; label: string } {
  if (vote >= 5) return { cls: "bg-success", label: "approved" };
  if (vote === -5) return { cls: "bg-warning", label: "waiting for author" };
  if (vote <= -10) return { cls: "bg-danger", label: "rejected" };
  return { cls: "bg-border-strong", label: "no vote yet" };
}

/** Work-item type colour, matching the board's swatches. */
const wiTypeColor: Record<string, string> = {
  Bug: "#e15b64",
  Task: "#d99e2b",
  "Product Backlog Item": "#2aa5e0",
  "User Story": "#2aa5e0",
  Feature: "#9a74d8",
  Epic: "#e0873c",
};

/** One linked work item as a DevOps-style chip: type-coloured icon, id,
 * title, and a state dot. Opens the work item in the browser. */
function WorkItemChip({ wi }: { wi: PrWorkItem }) {
  const color = wiTypeColor[wi.work_item_type] ?? "#9ca3af";
  return (
    <button
      className="flex w-full items-center gap-2 rounded-md border border-border bg-surface px-2.5 py-1.5 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
      title={`Open ${wi.work_item_type} ${wi.id} in Azure DevOps`}
      onClick={() => openUrl(wi.url).catch(() => toast.error("Could not open the browser."))}
    >
      {wi.work_item_type === "Bug" ? (
        <Bug size={13} className="shrink-0" style={{ color }} />
      ) : (
        <span
          className="inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
          style={{ backgroundColor: color }}
        />
      )}
      <span className="id-mono shrink-0 text-faint">{wi.id}</span>
      <span className="truncate text-text">{wi.title}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1 text-muted">
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ backgroundColor: wi.state_color ? `#${wi.state_color}` : "#9ca3af" }}
        />
        {wi.state}
      </span>
    </button>
  );
}

/** The latest run only - a summary, not a history. The dialog behind
 * "View history" is where the older runs and the step detail live. */
function BuildCard({ b, total }: { b: PrBuild; total: number }) {
  const when = b.started ? new Date(b.started).toLocaleString() : "";
  const failed = failurePath(b);
  const took = duration(b.started, b.finished);
  return (
    <div className="space-y-1.5 rounded-md border border-border bg-bg p-2">
      <div className="flex items-center gap-2">
        <span className={cn("rounded-full px-1.5 py-0.5 text-[10px] font-medium", tone(b.status, b.result))}>
          {label(b.result || b.status)}
        </span>
        <span className="truncate text-text">{b.name}</span>
        <span className="id-mono shrink-0 text-faint">{b.number}</span>
        <span className="shrink-0 rounded bg-surface-2 px-1 py-0.5 text-[10px] text-muted">
          {b.is_validation ? "PR validation" : "CI"}
        </span>
        {b.web_url && (
          <span
            role="button"
            aria-label={`Open build ${b.number} in Azure DevOps`}
            title="Open build in Azure DevOps"
            className="ml-auto shrink-0 rounded p-0.5 text-muted hover:text-accent"
            onClick={(e) => {
              e.stopPropagation();
              openUrl(b.web_url).catch(() => toast.error("Could not open the browser."));
            }}
          >
            <ExternalLink size={12} />
          </span>
        )}
      </div>
      {b.stages.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {b.stages.map((s, i) => (
            <span
              key={i}
              className={cn("rounded px-1.5 py-0.5 text-[10px]", tone(s.state, s.result))}
              title={`Stage ${s.name}: ${label(s.result || s.state)}`}
            >
              {s.name}
            </span>
          ))}
        </div>
      )}
      {/* Environments are the whole point of the feature - which of these
          did this change actually reach, and how far did it get. */}
      {b.deployments.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          <Rocket size={11} className="text-faint" />
          {b.deployments.map((d, i) => (
            <span
              key={i}
              className={cn("rounded-full px-1.5 py-0.5 text-[10px] font-medium", tone(d.status))}
              title={`${d.release} → ${d.environment}: ${label(d.status)}${
                d.on ? ` (${new Date(d.on).toLocaleString()})` : ""
              }`}
            >
              {d.environment}
            </span>
          ))}
        </div>
      )}
      {/* If it went red, say where without making the user open anything. */}
      {failed && <p className="text-danger">Failed at {failed}</p>}
      <p className="text-faint">
        {when}
        {took && ` · ${took}`}
        {total > 1 && ` · ${total - 1} earlier run${total === 2 ? "" : "s"}`}
      </p>
    </div>
  );
}

function PrRow({ pr, org, project }: { pr: PullRequest; org: string; project: string }) {
  const [open, setOpen] = useState(false);
  const [showPipeline, setShowPipeline] = useState(false);
  const created = pr.created ? new Date(pr.created).toLocaleDateString() : "";
  const closed = pr.closed ? new Date(pr.closed).toLocaleDateString() : "";
  // Linked work items load lazily, only when the row is expanded.
  const workItems = useQuery({
    queryKey: ["pr-work-items", org, project, pr.repo, pr.id],
    queryFn: () => unwrap(commands.prWorkItems(org, project, pr.repo, pr.id)),
    enabled: open && Boolean(org && project),
    staleTime: 5 * 60_000,
    retry: false,
  });
  // Builds + deployments, also lazy: several ADO calls per PR, so only for
  // the row the user actually opened.
  const pipeline = useQuery({
    queryKey: ["pr-pipeline", org, project, pr.repo_id, pr.id, pr.merge_commit],
    // repo_id, not repo: the Build API filters by repository GUID.
    queryFn: () => unwrap(commands.prPipeline(org, project, pr.repo_id, pr.id, pr.merge_commit)),
    enabled: open && Boolean(org && project),
    staleTime: 60_000,
    retry: false,
  });
  return (
    <div className="rounded-md border border-border bg-surface transition-colors hover:border-border-strong">
      {/* Clicking the row expands the detail; the external-link button is
          the way out to Azure DevOps. */}
      <button
        className="flex w-full items-start gap-3 px-3 py-2 text-left"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {open ? (
          <ChevronDown size={14} className="mt-1 shrink-0 text-muted" />
        ) : (
          <ChevronRight size={14} className="mt-1 shrink-0 text-muted" />
        )}
        <GitPullRequest size={15} className="mt-0.5 shrink-0 text-accent" />
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-2">
            <span className="truncate text-sm font-medium text-text">
              <span className="id-mono text-faint">!{pr.id}</span> {pr.title}
            </span>
            {pr.is_draft && (
              <span className="rounded-full bg-surface-2 px-2 py-0.5 text-[10px] font-medium text-muted">
                Draft
              </span>
            )}
            {pr.has_conflicts && (
              <span className="rounded-full bg-warning/15 px-2 py-0.5 text-[10px] font-medium text-warning">
                Conflicts
              </span>
            )}
          </span>
          <span className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
            {/* Same repo-pill treatment as the board's PR chips. */}
            <span className="rounded-full bg-accent-soft px-1.5 py-0.5 text-[10px] font-medium text-accent">
              {pr.repo}
            </span>
            <span className="flex items-center gap-1">
              <GitBranch size={11} />
              {pr.source_branch} → {pr.target_branch}
            </span>
            <span>{pr.author}</span>
          </span>
        </span>
        {/* Reviewer pips: one dot per reviewer, tinted by vote. */}
        <span className="mt-1 flex shrink-0 items-center gap-1">
          {pr.reviewers.map((r, i) => {
            const d = voteDot(r.vote);
            return (
              <span
                key={i}
                className={cn("inline-block h-2 w-2 rounded-full", d.cls)}
                title={`${r.display_name}: ${d.label}`}
              />
            );
          })}
        </span>
        <span
          role="button"
          aria-label={`Open !${pr.id} in Azure DevOps`}
          title="Open in Azure DevOps"
          className="mt-0.5 shrink-0 rounded p-1 text-muted hover:text-accent"
          onClick={(e) => {
            e.stopPropagation();
            openUrl(pr.web_url).catch(() => toast.error("Could not open the browser."));
          }}
        >
          <ExternalLink size={13} />
        </span>
      </button>

      {open && (
        <div className="space-y-2 border-t border-border/60 px-9 py-2 text-xs">
          {pr.description.trim() ? (
            // Astryx Markdown renders remote-authored text as React (no
            // dangerouslySetInnerHTML - PR descriptions are an XSS surface).
            // Links open in the system browser via the opener plugin.
            <AstryxIsland>
              <Markdown
                density="compact"
                autolink="gfm"
                contentWidth="100%"
                className="text-text"
                onLinkClick={(href) => {
                  openUrl(href).catch(() => toast.error("Could not open the browser."));
                  return false;
                }}
              >
                {pr.description}
              </Markdown>
            </AstryxIsland>
          ) : (
            <p className="text-faint">No description.</p>
          )}
          {/* Related work items, rendered as DevOps-style chips (icon + id +
              title + state) from the PR's real linkage, not the description. */}
          {(workItems.data?.length ?? 0) > 0 && (
            <div className="space-y-1 pt-1">
              <p className="font-semibold text-muted">Related work items</p>
              {workItems.data!.map((wi) => (
                <WorkItemChip key={wi.id} wi={wi} />
              ))}
            </div>
          )}
          <div className="space-y-1">
            {pr.reviewers.length === 0 && <p className="text-faint">No reviewers assigned.</p>}
            {pr.reviewers.map((r, i) => {
              const d = voteDot(r.vote);
              return (
                <p key={i} className="flex items-center gap-2 text-muted">
                  <span className={cn("inline-block h-2 w-2 rounded-full", d.cls)} />
                  {r.display_name}
                  <span className="text-faint">— {d.label}</span>
                </p>
              );
            })}
          </div>
          {/* Pipeline: which builds ran for this PR and where they got
              deployed. Best-effort - a PR with no pipeline just says so. */}
          <div className="space-y-1 pt-1">
            <div className="flex items-center gap-2">
              <p className="font-semibold text-muted">Last Run Pipeline</p>
              {(pipeline.data?.length ?? 0) > 0 && (
                <button
                  className="ml-auto rounded border border-border px-2 py-0.5 text-[11px] text-muted transition-colors hover:border-border-strong hover:text-text"
                  onClick={() => setShowPipeline(true)}
                >
                  View history
                </button>
              )}
            </div>
            {pipeline.isPending ? (
              <Skeleton className="h-10" />
            ) : pipeline.isError ? (
              // Show what Azure DevOps actually said - a bare "could not
              // read" hid a 400 from a bad repositoryId for a whole release.
              <p className="text-danger">{pipeline.error.message}</p>
            ) : (pipeline.data?.length ?? 0) === 0 ? (
              <p className="text-faint">No builds found for this pull request.</p>
            ) : (
              <BuildCard b={pipeline.data![0]} total={pipeline.data!.length} />
            )}
          </div>
          <div className="flex flex-wrap gap-x-3 text-faint">
            {created && <p>Created {created}</p>}
            {closed && <p>Closed {closed}</p>}
          </div>
        </div>
      )}

      {showPipeline && (
        <PipelineDialog
          prId={pr.id}
          prTitle={pr.title}
          repo={pr.repo}
          builds={pipeline.data ?? []}
          org={org}
          project={project}
          onClose={() => setShowPipeline(false)}
        />
      )}
    </div>
  );
}

function PrGroup({
  title,
  prs,
  emphasize = false,
  empty,
  org,
  project,
}: {
  title: string;
  prs: PullRequest[];
  emphasize?: boolean;
  empty: string;
  org: string;
  project: string;
}) {
  return (
    <section className="space-y-2">
      <h2
        className={cn(
          "text-sm font-semibold",
          emphasize && prs.length > 0 ? "text-accent" : "text-muted",
        )}
      >
        {title}
        {prs.length > 0 && <span className="ml-2 text-xs font-normal text-faint">{prs.length}</span>}
      </h2>
      {prs.length === 0 ? (
        <p className="rounded-md border border-dashed border-border px-3 py-2 text-xs text-faint">
          {empty}
        </p>
      ) : (
        <div className="space-y-1.5">
          {prs.map((pr) => (
            <PrRow key={`${pr.repo}-${pr.id}`} pr={pr} org={org} project={project} />
          ))}
        </div>
      )}
    </section>
  );
}

export default function PrPanel({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const repoKey = `tcm-v2-pr-repo:${org}/${project}`;
  const [repoId, setRepoIdRaw] = useState(() => localStorage.getItem(repoKey) ?? "");
  const setRepoId = (id: string) => {
    setRepoIdRaw(id);
    try {
      localStorage.setItem(repoKey, id);
    } catch {
      // session-only
    }
  };

  const overview = useQuery({
    queryKey: ["pr-overview", org, project],
    queryFn: () => unwrap(commands.prOverview(org, project)),
    enabled: Boolean(org && project),
    retry: false,
  });
  const repos = useQuery({
    queryKey: ["repos", org, project],
    queryFn: () => unwrap(commands.listRepos(org, project)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });
  // Active vs completed on the chosen repo. Completed is a separate ADO
  // query (capped server-side), not a client filter.
  const statusKey = "tcm-v2-pr-status";
  const [prStatus, setPrStatusRaw] = useState<"active" | "completed">(() =>
    localStorage.getItem(statusKey) === "completed" ? "completed" : "active",
  );
  const setPrStatus = (s: "active" | "completed") => {
    setPrStatusRaw(s);
    try {
      localStorage.setItem(statusKey, s);
    } catch {
      // session-only
    }
  };
  const active = useQuery({
    queryKey: ["repo-prs", org, project, repoId, prStatus],
    queryFn: () => unwrap(commands.repoPullRequests(org, project, repoId, prStatus)),
    enabled: Boolean(org && project && repoId),
    retry: false,
  });

  const repoName = repos.data?.find((r) => r.id === repoId)?.name;

  // "Active on <repo>" drops any PR already shown above (awaiting/yours), so
  // your own PRs in the selected repo appear once, under Your pull requests.
  const shownAbove = useMemo(() => {
    const ids = new Set<number>();
    for (const pr of overview.data?.awaiting ?? []) ids.add(pr.id);
    for (const pr of overview.data?.mine ?? []) ids.add(pr.id);
    return ids;
  }, [overview.data]);
  // Only the active list can collide with the groups above; completed PRs
  // are never shown there, so they must not be de-duplicated away.
  const repoPrs =
    prStatus === "active"
      ? (active.data ?? []).filter((pr) => !shownAbove.has(pr.id))
      : (active.data ?? []);

  return (
    <div className="max-w-3xl space-y-6">
      <div className="flex items-center gap-2">
        <Select
          aria-label="Repository"
          className="w-64 py-1.5"
          value={repoId}
          onChange={(e) => setRepoId(e.target.value)}
        >
          <option value="">Pick a repository…</option>
          {(repos.data ?? []).map((r) => (
            <option key={r.id} value={r.id}>
              {r.name}
            </option>
          ))}
        </Select>
        {/* Which slice of the repo's PRs the bottom group shows. */}
        <div className="flex rounded-md border border-border p-0.5">
          {(["active", "completed"] as const).map((s) => (
            <button
              key={s}
              className={cn(
                "rounded px-2 py-1 text-xs capitalize transition-colors",
                prStatus === s ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
              )}
              aria-pressed={prStatus === s}
              onClick={() => setPrStatus(s)}
            >
              {s}
            </button>
          ))}
        </div>
        <button
          aria-label="Refresh pull requests"
          title="Refresh pull requests"
          className="rounded p-1.5 text-muted transition-colors hover:text-accent"
          onClick={() => {
            qc.invalidateQueries({ queryKey: ["pr-overview", org, project] });
            qc.invalidateQueries({ queryKey: ["repo-prs", org, project, repoId] });
          }}
        >
          <RefreshCw
            size={14}
            className={overview.isFetching || active.isFetching ? "animate-spin" : undefined}
          />
        </button>
      </div>

      {overview.isError && <p className="text-sm text-danger">{overview.error.message}</p>}
      {overview.isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-14" />
          <Skeleton className="h-14" />
        </div>
      )}

      {overview.data && (
        <>
          <PrGroup
            title="Awaiting your review"
            prs={overview.data.awaiting}
            emphasize
            empty="Nothing waiting on you."
            org={org}
            project={project}
          />
          <PrGroup
            title="Your pull requests"
            prs={overview.data.mine}
            empty="You have no active pull requests."
            org={org}
            project={project}
          />
        </>
      )}

      {repoId &&
        (active.isError ? (
          <p className="text-sm text-danger">{active.error.message}</p>
        ) : (
          <PrGroup
            title={`${prStatus === "active" ? "Active" : "Completed"} on ${
              repoName ?? "repository"
            }`}
            prs={repoPrs}
            empty={
              active.isLoading
                ? "Loading…"
                : prStatus === "active"
                  ? "No other active pull requests on this repository."
                  : "No completed pull requests on this repository."
            }
            org={org}
            project={project}
          />
        ))}
    </div>
  );
}
