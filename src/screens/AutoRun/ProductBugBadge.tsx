// The label a case gets when a product bug (a quirk of that class) names it:
// the application is wrong and the case works around it. Shared by the
// review dialog and Past runs, like NoticeBadge.

import { Badge } from "../../components/ui/badge";

/** Nothing for a case with no product bug. The label's title lists them. */
export default function ProductBugBadge({ bugs }: { bugs?: string[] | null }) {
  if (!bugs || bugs.length === 0) return null;
  return (
    <Badge className="shrink-0 bg-danger/15 text-danger" title={bugs.join("\n")}>
      Product bug
    </Badge>
  );
}
