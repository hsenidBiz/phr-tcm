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

export function reportUpdateCheck(status: UpdateStatus): void {
  if (status.available) {
    toast.info(`Version ${status.available} is available - use the banner to update.`);
  } else if (status.blocked) {
    toast.warning(`Could not check for updates: ${status.blocked}`, { duration: 10_000 });
  } else {
    toast.success("You are on the latest version.");
  }
}
