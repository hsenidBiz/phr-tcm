// The Playwright tab's health: a row of badges in the header, one per thing
// stopping the rest of the tab working, each with what to do about it on
// hover. A clone with nothing wrong shows one "All good" badge.

import type { HealthIssue } from "../../bindings";
import { StatusBadge } from "./parts";

/** What each issue says in full, and what to do. Exported for the tests. */
export const HEALTH_TEXT: Record<HealthIssue, string> = {
  NotTheRepo: "This folder is not a clone of the team's Playwright repo.",
  NoNodeModules: "The clone has no node_modules: run npm install in it.",
  NoNode: "Node was not found on this machine: install Node.js, then restart the app.",
  NoQaOptions: "No qa-options.json in the clone: runs use the default options.",
};

/** Each issue on its badge. Exported for the tests. */
export const HEALTH_LABEL: Record<HealthIssue, string> = {
  NotTheRepo: "Not the repo",
  NoNodeModules: "No node_modules",
  NoNode: "Node not found",
  NoQaOptions: "No qa-options.json",
};

/** A missing qa-options.json only means defaults; everything else stops the tab. */
export const WARNING_ONLY: ReadonlySet<HealthIssue> = new Set<HealthIssue>(["NoQaOptions"]);

export default function HealthLine({ issues }: { issues: HealthIssue[] }) {
  return (
    <div role="status" aria-label="Repo health" className="flex flex-wrap items-center gap-1.5">
      {issues.length === 0 ? (
        <StatusBadge tone="success" title="The clone is ready.">
          All good
        </StatusBadge>
      ) : (
        issues.map((i) => (
          <StatusBadge key={i} tone={WARNING_ONLY.has(i) ? "warning" : "danger"} title={HEALTH_TEXT[i]}>
            {HEALTH_LABEL[i]}
          </StatusBadge>
        ))
      )}
    </div>
  );
}
