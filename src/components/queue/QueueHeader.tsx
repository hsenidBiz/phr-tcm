import { OFFLINE_HINT } from "../../lib/network";
import { IconOpenInBrowser, IconRemove, IconShare } from "../../lib/actionIcons";
import { Button } from "../ui/button";

/** The "Queue for PBI" heading and its three actions. */
export default function QueueHeader({
  pbiId,
  queueLength,
  online,
  viewPending,
  sharePending,
  submitPending,
  onView,
  onShare,
  onClear,
}: {
  pbiId: number;
  queueLength: number;
  online: boolean;
  viewPending: boolean;
  sharePending: boolean;
  submitPending: boolean;
  onView: () => void;
  onShare: () => void;
  onClear: () => void;
}) {
  return (
    <div className="flex items-center justify-between">
      <h2 className="text-sm font-semibold text-text">
        Queue for PBI #{pbiId} ({queueLength} queued)
      </h2>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={queueLength === 0 || viewPending}
          onClick={onView}
        >
          <IconOpenInBrowser aria-hidden />
          View in browser
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={queueLength === 0 || sharePending || !online}
          title={online ? "Upload the draft as a one-time share link a colleague can import for review" : OFFLINE_HINT}
          onClick={onShare}
        >
          <IconShare aria-hidden />
          {sharePending ? "Sharing" : "Share for review"}
        </Button>
        {/* Last on purpose, and red on approach: this is the destroy
            action in a row of build actions, so it sits at the far end
            where a fast hand does not land on it by habit, and announces
            itself before the click. */}
        <Button
          variant="outline"
          size="sm"
          className="hover:border-danger hover:bg-danger/10 hover:text-danger"
          disabled={queueLength === 0 || submitPending}
          onClick={onClear}
        >
          <IconRemove aria-hidden />
          Clear queue
        </Button>
      </div>
    </div>
  );
}
