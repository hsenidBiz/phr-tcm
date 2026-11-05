// The small label on a failed case whose script the person marked Flaky. Shared by the review dialog and Past runs.

import { Badge } from "../../components/ui/badge";

export default function FlakyBadge() {
  return (
    <Badge
      className="shrink-0 bg-warning/15 text-warning"
      title="Marked as flaky: it passes and fails on its own. The failure is still sent as failed."
    >
      Flaky
    </Badge>
  );
}
