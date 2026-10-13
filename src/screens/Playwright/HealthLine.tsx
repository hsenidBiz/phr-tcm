// The Playwright tab's health line: one line that names anything stopping
// the rest of the tab working, with what to do about it.

import type { HealthIssue } from "../../bindings";

/** What each issue says. Exported for the tests. */
export const HEALTH_TEXT: Record<HealthIssue, string> = {
  NotTheRepo: "This folder is not a clone of the team's Playwright repo.",
  NoNodeModules: "The clone has no node_modules: run npm install in it.",
  NoNode: "Node was not found on this machine: install Node.js, then restart the app.",
  NoQaOptions: "No qa-options.json in the clone: runs use the default options.",
};

/** A missing qa-options.json only means defaults; everything else stops the tab. */
const WARNING_ONLY: ReadonlySet<HealthIssue> = new Set<HealthIssue>(["NoQaOptions"]);

export default function HealthLine({ issues }: { issues: HealthIssue[] }) {
  if (issues.length === 0) {
    return (
      <p role="status" aria-label="Repo health" className="text-xs text-success">
        The clone is ready.
      </p>
    );
  }
  return (
    <p role="status" aria-label="Repo health" className="flex flex-wrap gap-x-3 gap-y-1 text-xs">
      {issues.map((i) => (
        <span key={i} className={WARNING_ONLY.has(i) ? "text-warning" : "text-danger"}>
          {HEALTH_TEXT[i]}
        </span>
      ))}
    </p>
  );
}
