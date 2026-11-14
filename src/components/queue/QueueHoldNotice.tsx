import { OFFLINE_HINT } from "../../lib/network";
import { saveHold, type UploadHold } from "../../lib/uploadHold";
import { IconCancel, IconRefresh, IconRelease } from "../../lib/actionIcons";
import { Button } from "../ui/button";
import { Modal } from "../ui/modal";

/** The "outcome unknown" panel for creates an interrupted upload left in
 * doubt, with its Check and Release actions and the Release confirmation. */
export default function QueueHoldNotice({
  hold,
  online,
  checkingHold,
  onCheck,
  onRelease,
}: {
  hold: UploadHold;
  online: boolean;
  checkingHold: boolean;
  onCheck: () => void;
  onRelease: () => void;
}) {
  return (
    <div className="space-y-2 rounded-md border border-warning/50 bg-warning/10 p-3">
      {hold.ambiguous && hold.ambiguous.length > 0 ? (
        <>
          <p className="text-sm font-semibold text-text">
            More than one test case with these titles exists in Azure DevOps. Check there, then
            release them here.
          </p>
          <p className="text-xs text-muted">
            Azure DevOps has more matches for {hold.ambiguous.length === 1 ? "this title" : "these titles"}{" "}
            than rows waiting on it, so this app cannot tell which one is genuinely this upload's.
            Once you have confirmed it there, Release lets{" "}
            {hold.ambiguous.length === 1 ? "that case" : "those cases"} be uploaded again.
          </p>
        </>
      ) : (
        <>
          <p className="text-sm font-semibold text-text">
            Outcome unknown for {hold.titles.length} case{hold.titles.length === 1 ? "" : "s"} - check
            before uploading again.
          </p>
          <p className="text-xs text-muted">
            The last upload was interrupted and Azure DevOps could not be asked what it had created.
            Uploading again before checking could create {hold.titles.length === 1 ? "it" : "them"} twice.
          </p>
        </>
      )}
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          disabled={checkingHold || !online}
          title={online ? undefined : OFFLINE_HINT}
          onClick={onCheck}
        >
          <IconRefresh aria-hidden />
          {checkingHold ? "Checking" : "Check with Azure DevOps"}
        </Button>
        {hold.ambiguous && hold.ambiguous.length > 0 && (
          <Button size="sm" variant="outline" onClick={onRelease}>
            <IconRelease aria-hidden />
            Release
          </Button>
        )}
      </div>
    </div>
  );
}

/** The confirmation before held cases are released for upload again. */
export function ReleaseConfirmModal({
  org,
  pbiId,
  onClose,
}: {
  org: string;
  pbiId: number;
  onClose: () => void;
}) {
  return (
    <Modal onClose={onClose} className="w-full max-w-md space-y-4 p-5">
      <h2 className="text-sm font-semibold text-text">Release held cases?</h2>
      <p className="text-xs text-muted">
        I have checked in Azure DevOps — release these cases so they can be uploaded again?
      </p>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          onClick={() => {
            saveHold(org, pbiId, null);
            onClose();
          }}
        >
          <IconRelease aria-hidden />
          Release
        </Button>
      </div>
    </Modal>
  );
}
