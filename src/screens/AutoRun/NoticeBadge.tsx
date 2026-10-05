// The small label a case gets when the run skipped something for it and
// it went on anyway - today, preconditions not checked while Database Read
// Access was off. Shared by the review dialog and Past runs, like
// RetriedBadge.

import { Badge } from "../../components/ui/badge";

/** Nothing for a case with no notice. The label's title carries the
 * sentence. */
export default function NoticeBadge({ notice }: { notice?: string | null }) {
  if (!notice) return null;
  return (
    <Badge className="shrink-0 bg-warning/15 text-warning" title={notice}>
      Not checked
    </Badge>
  );
}
