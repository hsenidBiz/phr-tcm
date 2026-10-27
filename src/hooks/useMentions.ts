/**
 * Work-item @mentions of you, checked when the app starts and every five
 * minutes after, into the bell (and a toast or OS notification, like a new
 * assignment). Mounted beside usePrAttention, which does the same for
 * mentions in PR threads. A failed check is logged and simply tried again
 * at the next one - no toast.
 */
import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { commands } from "../bindings";
import { unwrap } from "../lib/ipc";
import { announceMentions, noteMentions, noteMentionsAcross, workItemNotification } from "../lib/mentions";
import { logUi } from "../lib/uiLog";

export const MENTIONS_POLL_MS = 5 * 60_000;

/** `project` is the picked project, or the home project while the projects
 * show together (`allProjects`): then one check covers the whole organization
 * and `project` only fills in for a mention that names none. */
export function useMentions(org: string, project: string, allProjects: boolean): void {
  const mentions = useQuery({
    queryKey: allProjects ? ["recent-mentions-all", org] : ["recent-mentions", org, project],
    queryFn: async () =>
      (await unwrap(allProjects ? commands.recentMentionsAll(org) : commands.recentMentions(org, project))) ?? [],
    enabled: allProjects ? Boolean(org) : Boolean(org && project),
    refetchInterval: MENTIONS_POLL_MS,
    // A minimised app still checks: that is when the OS notification is
    // the only way the mention is seen.
    refetchIntervalInBackground: true,
    retry: false,
  });

  useEffect(() => {
    if (!mentions.data) return;
    // The tour guard lives in noteMentions itself (the write chokepoint),
    // not here - see mentions.ts.
    const found = mentions.data.map((m) => ({
      notification: workItemNotification(org, project, m),
      created: m.created_date,
    }));
    // Together, the first check learns the other projects' standing mentions
    // instead of raising them; `project` is then the home project.
    announceMentions(allProjects ? noteMentionsAcross(org, project, found) : noteMentions(org, found));
  }, [mentions.data, org, project, allProjects]);

  useEffect(() => {
    if (mentions.error) {
      logUi(`mentions: work-item check failed, trying again at the next check: ${mentions.error.message}`);
    }
  }, [mentions.error, mentions.errorUpdatedAt]);
}
