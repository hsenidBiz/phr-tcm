// The small label a case gets when its script flags page errors
// (`"page_errors": "flag"`) and the page had some: uncaught script errors
// or 5xx answers, counted, not failed. Shared by the review dialog and Past
// runs, like RetriedBadge.

import { Badge } from "../../components/ui/badge";

/** Nothing for a case that met none. The title says what was counted. */
export default function PageErrorsBadge({ seen }: { seen?: number | null }) {
  if (!seen) return null;
  return (
    <Badge
      className="shrink-0 bg-warning/15 text-warning"
      title="Uncaught script errors and 5xx answers the page had while the case ran - each one is listed in its step"
    >
      page errors seen: {seen}
    </Badge>
  );
}
