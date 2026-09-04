/**
 * What a manual "Check for updates" says, in one place.
 *
 * There are three outcomes, not two: a newer version, genuinely up to date,
 * and "no check happened" (this build does not self-update, or the feed was
 * unreachable). The last one used to be reported as "You are on the latest
 * version" - a claim the app had not checked and could not make, told to
 * exactly the person most likely to be running something stale.
 */
import { toast } from "sonner";
import type { UpdateStatus } from "../bindings";

/** What a user who cannot read the releases repo is told. One place, so
 * the banner in App and the toast here cannot drift apart. */
export const UPDATES_MOVED = {
  title: "App updates have moved to Azure DevOps.",
  body: "You don't currently have access to the PHR-TCM repository, so you won't receive the latest updates. Please raise a Redmine ticket asking for read access to HRM / PHR-TCM.",
} as const;

export function reportUpdateCheck(status: UpdateStatus): void {
  if (status.available) {
    toast.info(`Version ${status.available} is available - use the banner to update.`);
    // A blocked check is reported generically - except when the reason IS
    // the missing access, because then the notice below already says it,
    // and saying both stacked three clauses on one line: "Could not check
    // for updates: could not reach the update feed: you don't have access
    // yet...". The reader had to get past two of our sentences to reach
    // the one telling them what to do.
  } else if (status.blocked && !status.no_access) {
    toast.warning(`Could not check for updates: ${status.blocked}`, { duration: 10_000 });
  } else if (!status.blocked && !status.no_access) {
    toast.success("You are on the latest version.");
  }
  // Told on top of whichever of the above applied: access being missing
  // is true regardless of what GitHub managed to serve.
  if (status.no_access) {
    toast.warning(`${UPDATES_MOVED.title} ${UPDATES_MOVED.body}`, { duration: 15_000 });
  }
}
