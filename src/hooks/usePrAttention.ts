/**
 * How many of the user's pull requests need a human right now - merge
 * conflicts or unresolved review comments - for the Work Manager badge.
 *
 * Same query keys and stale times as the PR panel's own rows, so the two
 * share one cache: opening the panel doesn't refetch what the badge just
 * asked, and the badge stays honest while the panel is open. Scope is the
 * overview's slices (PRs you created + PRs awaiting your review), which
 * are active by definition - closed PRs have nothing left to resolve.
 */
import { useQueries, useQuery } from "@tanstack/react-query";
import { commands, type PullRequest } from "../bindings";
import { isResolved } from "../components/PrThreads";
import { unwrap } from "../lib/ipc";

/** Background refresh - a badge that only updates on tab focus goes stale
 * exactly when the user is heads-down elsewhere in the app. */
const POLL_MS = 5 * 60_000;

export function usePrAttention(org: string, project: string): number {
  const overview = useQuery({
    queryKey: ["pr-overview", org, project],
    queryFn: () => unwrap(commands.prOverview(org, project)),
    enabled: Boolean(org && project),
    refetchInterval: POLL_MS,
    retry: false,
  });

  // A PR can be in both slices (own PR, also a listed reviewer) - count it once.
  const prs: PullRequest[] = [];
  const seen = new Set<number>();
  for (const pr of [...(overview.data?.mine ?? []), ...(overview.data?.awaiting ?? [])]) {
    if (!seen.has(pr.id)) {
      seen.add(pr.id);
      prs.push(pr);
    }
  }

  const threads = useQueries({
    queries: prs.map((pr) => ({
      queryKey: ["pr-threads", org, project, pr.repo, pr.id],
      queryFn: () => unwrap(commands.prThreads(org, project, pr.repo, pr.id)),
      staleTime: 2 * 60_000,
      refetchInterval: POLL_MS,
      retry: false,
    })),
  });

  return prs.filter((pr, i) => {
    const unresolved = (threads[i]?.data ?? []).filter((t) => !isResolved(t.status)).length;
    return pr.has_conflicts || unresolved > 0;
  }).length;
}
