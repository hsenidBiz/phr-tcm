// Rename folder, when the rename would break something in the repo: the list
// of what, and the choice to go ahead. The app never rewrites the repo's
// files to mend a path - it says what will break, and the person fixes it
// in the repository. Its own Export to Playwright settings for the folder
// move with it (Rust's pw_repo::rename_folder), which the dialog says.

import { useId } from "react";
import type { RenameCheck } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconRename } from "../../lib/actionIcons";

export default function RenameFolderDialog({
  check,
  busy,
  error,
  onConfirm,
  onCancel,
}: {
  check: RenameCheck;
  busy: boolean;
  error: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const heading = useId();
  return (
    <Modal onClose={onCancel} labelledBy={heading} className="flex max-h-[85vh] w-full max-w-lg flex-col gap-4 p-5">
      <h2 id={heading} className="shrink-0 text-sm font-semibold text-text">
        Rename this folder?
      </h2>
      <p className="shrink-0 text-xs text-muted">
        {`Renaming ${check.from} to ${check.to} will break these, which you will need to fix in the repository:`}
      </p>
      <ul aria-label="What would break" className="min-h-0 flex-1 space-y-1 overflow-auto text-xs">
        {check.breaks.map((b, i) => (
          <li key={`${b.file}-${i}`} className="rounded border border-border px-2 py-1.5">
            <span className="id-mono break-all text-text">{b.file}</span>
            <span className="block text-muted">{b.what}</span>
          </li>
        ))}
      </ul>
      {check.tcm_updates.length > 0 && (
        <p className="shrink-0 text-xs text-muted">TCM&apos;s own export settings for this folder will be updated.</p>
      )}
      {error && <p className="shrink-0 text-xs text-danger">{error}</p>}
      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={busy} onClick={onCancel}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={busy} onClick={onConfirm}>
          <IconRename aria-hidden />
          Rename anyway
        </Button>
      </div>
    </Modal>
  );
}
