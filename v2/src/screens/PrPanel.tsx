// The Work Manager's Pull Requests panel: three read-only groups ordered
// by actionability - awaiting your review, yours, then everything active
// on a chosen repo. Rows open the PR in the browser; voting/completing
// stays in Azure DevOps (this panel never writes).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { GitBranch, GitPullRequest, RefreshCw } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type PullRequest } from "../bindings";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";

/** ADO reviewer votes: 10 approved, 5 approved w/ suggestions, 0 waiting,
 * -5 waiting for author, -10 rejected. */
function voteDot(vote: number): { cls: string; label: string } {
  if (vote >= 5) return { cls: "bg-success", label: "approved" };
  if (vote === -5) return { cls: "bg-warning", label: "waiting for author" };
  if (vote <= -10) return { cls: "bg-danger", label: "rejected" };
  return { cls: "bg-border-strong", label: "no vote yet" };
}

function PrRow({ pr }: { pr: PullRequest }) {
  return (
    <button
      className="flex w-full items-start gap-3 rounded-md border border-border bg-surface px-3 py-2 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
      onClick={() => openUrl(pr.web_url).catch(() => toast.error("Could not open the browser."))}
      title="Open in Azure DevOps"
    >
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
          <span>{pr.repo}</span>
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
    </button>
  );
}

function PrGroup({
  title,
  prs,
  emphasize = false,
  empty,
}: {
  title: string;
  prs: PullRequest[];
  emphasize?: boolean;
  empty: string;
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
            <PrRow key={`${pr.repo}-${pr.id}`} pr={pr} />
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
  const active = useQuery({
    queryKey: ["repo-prs", org, project, repoId],
    queryFn: () => unwrap(commands.repoPullRequests(org, project, repoId)),
    enabled: Boolean(org && project && repoId),
    retry: false,
  });

  const repoName = repos.data?.find((r) => r.id === repoId)?.name;

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
          />
          <PrGroup
            title="Your pull requests"
            prs={overview.data.mine}
            empty="You have no active pull requests."
          />
        </>
      )}

      {repoId &&
        (active.isError ? (
          <p className="text-sm text-danger">{active.error.message}</p>
        ) : (
          <PrGroup
            title={`Active on ${repoName ?? "repository"}`}
            prs={active.data ?? []}
            empty={active.isLoading ? "Loading…" : "No active pull requests on this repository."}
          />
        ))}
    </div>
  );
}
