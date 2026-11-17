// The bugs filed for a case from its run's review (Raise bug): "Bug #123", each opening that work item in
// Azure DevOps. Read off the case's saved record (`CaseRecord::bugs`), so it shows in the review and in
// Past runs alike, whenever they are opened. Shared by both, like FlakyBadge.

import { openUrl } from "@tauri-apps/plugin-opener";
import type { CaseBug } from "../../bindings";
import { toast } from "../../lib/toast";

/** The work item's page, the way every other place in the app opens one. */
function bugUrl(b: CaseBug): string {
  return `https://dev.azure.com/${encodeURIComponent(b.org)}/${encodeURIComponent(b.project)}/_workitems/edit/${b.id}`;
}

/** Nothing for a case with no filed bug. */
export default function FiledBugs({ bugs }: { bugs?: readonly CaseBug[] | null }) {
  if (!bugs || bugs.length === 0) return null;
  return (
    <>
      {bugs.map((b) => (
        <button
          key={b.id}
          type="button"
          className="shrink-0 rounded text-xs text-success underline-offset-2 hover:underline"
          title={`Open bug #${b.id} in Azure DevOps`}
          onClick={() => void openUrl(bugUrl(b)).catch(() => toast.error("Could not open the browser."))}
        >
          Bug #{b.id}
        </button>
      ))}
    </>
  );
}
