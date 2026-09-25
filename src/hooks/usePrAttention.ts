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
import { useEffect } from "react";
import { commands, type PullRequest } from "../bindings";
import { isResolved } from "../lib/threadStatus";
import { unwrap } from "../lib/ipc";
import { announceMentions, noteMentions, prMentions, prNotification } from "../lib/mentions";
import { notePrComments, notePrOverview } from "../lib/notifications";
import { logUi } from "../lib/uiLog";

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

  // The bell learns from the same fetch: your PRs that have grown
  // conflicts, PRs newly waiting on you. The store dedupes, so reporting
  // the whole overview on every poll raises each only once.
  useEffect(() => {
    if (overview.data) notePrOverview(org, project, overview.data);
  }, [overview.data, org, project]);

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

  const unresolvedFor = (i: number) =>
    (threads[i]?.data ?? []).filter((t) => !isResolved(t.status)).length;

  // Comments to resolve go to the bell as well - the number that used to
  // ride on the Work Manager pill. Keyed on the count in the store, so
  // this can run on every thread refresh and only a changed count raises.
  const signature = prs.map((pr, i) => `${pr.repo}:${pr.id}:${unresolvedFor(i)}`).join("|");
  useEffect(() => {
    prs.forEach((pr, i) => notePrComments(org, project, pr, unresolvedFor(i)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, org, project]);

  // Who "you" are, for the mention scan: the same in-memory, once-per-org
  // lookup the comments panel uses. `staleTime: Infinity` means it is
  // fetched once and left alone on success - but a failure must not sit
  // there forever, or the scan below silently stops for the rest of the
  // session; retry it on the same interval as every other mention check
  // until it succeeds.
  const me = useQuery({
    queryKey: ["connected-user", org],
    queryFn: async () => (await unwrap(commands.connectedUser(org))) ?? null,
    enabled: Boolean(org),
    staleTime: Infinity,
    refetchInterval: (q) => (q.state.status === "error" ? POLL_MS : false),
    retry: false,
  });
  const myId = me.data?.id ?? "";

  useEffect(() => {
    if (me.error) {
      logUi(`mentions: PR identity check failed, trying again at the next check: ${me.error.message}`);
    }
  }, [me.error, me.errorUpdatedAt]);

  // Mentions of you in these same threads, on every thread refresh - no
  // request of their own. The store dedupes, so a rescan raises only
  // what is new. The tour guard lives in noteMentions itself (the write
  // chokepoint), not here - see mentions.ts.
  //
  // Skipped while the identity query is unsettled: `reSignIn` invalidates
  // `connected-user`, `pr-overview` and `pr-threads` together, and a
  // thread result that lands before identity does would otherwise scan
  // with the PREVIOUS account's id. `isFetching` alone is not enough - a
  // failed refetch keeps the old `data` and would resume scanning with the
  // stale id on the very next thread refresh - so `isError` gates it too.
  // The existing retry interval on `me` (above) recovers from that state.
  const threadStamp = threads.map((t) => t.dataUpdatedAt).join("|");
  useEffect(() => {
    if (!myId || me.isFetching || me.isError) return;
    const found = prs.flatMap((pr, i) =>
      prMentions(pr, threads[i]?.data ?? [], myId).map((m) => ({
        notification: prNotification(org, project, m),
        created: m.createdDate,
      })),
    );
    announceMentions(noteMentions(org, found));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadStamp, myId, me.isFetching, me.isError, me.dataUpdatedAt, org, project]);

  return prs.filter((pr, i) => pr.has_conflicts || unresolvedFor(i) > 0).length;
}
