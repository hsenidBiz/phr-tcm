// The small label a case gets when an unattended run ran it a second time
// after a transient failure. Shared by the review dialog and Past runs.

import { Badge } from "../../components/ui/badge";

/** Nothing for a case that ran once. `first` is the first try's failure
 * sentence, which the label's title carries. */
export default function RetriedBadge({ first }: { first?: string | null }) {
  if (!first) return null;
  return (
    <Badge
      className="shrink-0 bg-warning/15 text-warning"
      title={`Run a second time after a transient failure: ${first}`}
    >
      Retried
    </Badge>
  );
}
