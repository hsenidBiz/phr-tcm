import { useState } from "react";
import type { TestCase } from "../bindings";
import {
  BulkFields,
  BulkSummary,
  NO_BULK_CHANGE,
  bulkChangedFields,
  bulkPreconditions,
  bulkTags,
} from "./BulkFields";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";
import { IconCancel, IconConfirm } from "../lib/actionIcons";

/** Bulk edit for selected QUEUE drafts. The sibling of BulkEditDialog,
 * deliberately separate: that one updates real Azure DevOps work items,
 * one API call per case; this one hands back a pure edit function the
 * queue applies in memory - the caller then writes the owning .json files
 * so the file says what the queue says. Every field defaults to "leave
 * unchanged"; titles and steps are never touched (titles have their own
 * tool - Power Rename). */
export default function QueueBulkEditDialog({
  org,
  project,
  count,
  onClose,
  onApply,
}: {
  org: string;
  project: string;
  count: number;
  onClose: () => void;
  onApply: (edit: (tc: TestCase) => TestCase) => void;
}) {
  const [values, setValues] = useState(NO_BULK_CHANGE);

  const nothingChosen = bulkChangedFields(values).length === 0;

  const buildEdit = (): ((tc: TestCase) => TestCase) => {
    // Captured once, so the edit is the same pure function for every case.
    const chosen = values;
    return (tc) => ({
      ...tc,
      tags: bulkTags(chosen, tc.tags),
      automation_status: chosen.status || tc.automation_status,
      priority: chosen.priority ?? tc.priority ?? null,
      module_value: chosen.moduleValue || tc.module_value,
      preconditions: bulkPreconditions(chosen, tc.preconditions),
    });
  };

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-md flex-col gap-4 p-5">
      <div className="shrink-0">
        <h2 className="text-sm font-semibold text-text">
          Bulk edit {count} queued draft{count === 1 ? "" : "s"}
        </h2>
        <p className="text-xs text-muted">
          Only the fields you set here change - titles and steps are never touched. The
          .json file each case came from is updated to match.
        </p>
      </div>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
        <BulkFields org={org} project={project} value={values} onChange={setValues} />
      </div>

      <BulkSummary value={values} />

      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={nothingChosen} onClick={() => onApply(buildEdit())}>
          <IconConfirm aria-hidden />
          Apply to {count}
        </Button>
      </div>
    </Modal>
  );
}
