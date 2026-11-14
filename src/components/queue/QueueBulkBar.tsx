import type { Dispatch, SetStateAction } from "react";
import { IconConfirm, IconRemove, IconRename } from "../../lib/actionIcons";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";

/** Bulk actions over a selection. Shift+click a checkbox to select a
 * range. Every action here also updates the .json file each case
 * came from - the file and the queue must not disagree. */
export default function QueueBulkBar({
  queueLength,
  selected,
  setSelected,
  onBulkEdit,
  onRename,
  onRemove,
}: {
  queueLength: number;
  selected: Set<number>;
  setSelected: Dispatch<SetStateAction<Set<number>>>;
  onBulkEdit: () => void;
  onRename: () => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface-2/50 px-3 py-1.5 text-xs">
      <label className="flex items-center gap-2 text-muted">
        <Checkbox
          ariaLabel="Select all queued cases"
          checked={selected.size === queueLength && queueLength > 0}
          onCheckedChange={(v) =>
            setSelected(v ? new Set(Array.from({ length: queueLength }, (_, i) => i)) : new Set())
          }
        />
        {selected.size > 0
          ? `${selected.size} of ${queueLength} selected`
          : "Select cases for bulk actions"}
      </label>
      {selected.size > 0 && (
        <>
          <Button variant="outline" size="sm" onClick={onBulkEdit}>
            <IconConfirm aria-hidden />
            Bulk edit
          </Button>
          <Button variant="outline" size="sm" onClick={onRename}>
            <IconRename aria-hidden />
            Rename {selected.size}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="hover:border-danger hover:bg-danger/10 hover:text-danger"
            onClick={onRemove}
          >
            <IconRemove aria-hidden />
            Remove {selected.size}
          </Button>
        </>
      )}
    </div>
  );
}
