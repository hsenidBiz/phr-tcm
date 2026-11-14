import type { SubmitItemResult } from "../../bindings";
import { toast } from "../../lib/toast";
import { cn } from "../../lib/cn";
import { copyText } from "../../lib/clipboard";
import { summariseSubmit } from "../../lib/submitSummary";
import { IconClear, IconCopy } from "../../lib/actionIcons";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";

/** What just happened, as a panel rather than a paragraph of coloured
 * text pasted under the queue. The headline answers "did it work"
 * on its own; the rows below are for finding one case. */
export default function QueueResults({
  results,
  changeNotes,
  onClear,
}: {
  results: SubmitItemResult[];
  changeNotes: string | null;
  onClear: () => void;
}) {
  return (
    <div className="space-y-2 rounded-md border border-border bg-surface-2 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-sm font-semibold text-text">
          {summariseSubmit(results).headline}
        </h3>
        {/* Dismiss the results once read - the button goes with them,
            and so do the marks on the rows: once the results are
            gone, the queue reads as a plain queue again. */}
        <div className="flex shrink-0 items-center gap-2">
          {/* For the tester: every updated case by id with what changed,
              then the new ones - so they can tell whether a case they
              already ran needs running again. */}
          {changeNotes && (
            <Button
              variant="outline"
              size="sm"
              title="Copy the updated test case ids and what changed, to send to a tester"
              onClick={() => {
                copyText(changeNotes)
                  .then(() => toast.success("Changes copied."))
                  .catch(() => toast.error("Could not copy to clipboard."));
              }}
            >
              <IconCopy aria-hidden />
              Copy changes
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={onClear}>
            <IconClear aria-hidden />
            Clear results
          </Button>
        </div>
      </div>
      <ul className="space-y-1 text-sm">
        {results.map((r) => (
          <li key={r.index} className="flex items-baseline gap-2">
            <Badge
              className={cn(
                "shrink-0",
                r.action === "created"
                  ? "bg-success/20 text-success"
                  : r.action === "updated" || r.action === "unknown"
                    ? "bg-warning/20 text-warning"
                    : "bg-danger/20 text-danger",
              )}
            >
              {r.action === "created"
                ? "New"
                : r.action === "updated"
                  ? "Updated"
                  : r.action === "unknown"
                    ? "Unknown"
                    : "Failed"}
            </Badge>
            {r.id != null && <span className="id-mono shrink-0 text-faint">#{r.id}</span>}
            <span className="min-w-0 break-words text-text">{r.title}</span>
            {/* A case that WAS written and then hit trouble afterwards
                still exists in Azure DevOps, so its note is a warning
                beside it, not a failure badge in front of it. */}
            {r.error && (
              <span
                className={cn(
                  "min-w-0 break-words",
                  r.action === "failed" ? "text-danger" : "text-warning",
                )}
              >
                {r.error}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
