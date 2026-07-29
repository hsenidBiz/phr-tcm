import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type DeleteOutcome } from "../bindings";
import { unwrap } from "../lib/ipc";
import { IconCancel, IconRemove } from "../lib/actionIcons";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";

/**
 * The confirmation for the one action this app cannot undo from inside.
 *
 * Everything else it does is a create or an update, and a mistake can be
 * edited back. A delete cannot - the recycle bin is Azure DevOps' to
 * restore from, not this app's. So the whole list is shown by id and title
 * rather than a count: "delete 12 test cases" is not something anyone can
 * check, and the point of a confirmation is to be checkable.
 */
export default function DeleteConfirm({
  org,
  project,
  cases,
  onClose,
  onDeleted,
}: {
  org: string;
  project: string;
  cases: { id: number; title: string }[];
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [failures, setFailures] = useState<DeleteOutcome[] | null>(null);

  const remove = useMutation({
    mutationFn: () => unwrap(commands.deleteTestCases(org, project, cases.map((c) => c.id))),
    onSuccess: (outcomes) => {
      const failed = outcomes.filter((o) => !o.deleted);
      const gone = outcomes.length - failed.length;
      if (failed.length === 0) {
        toast.success(
          `Moved ${gone} test case${gone === 1 ? "" : "s"} to the recycle bin in Azure DevOps.`,
        );
        onDeleted();
        onClose();
        return;
      }
      // Stay open and show WHICH survived. Closing here would leave the
      // user to work out from a refreshed list what had happened.
      setFailures(failed);
      onDeleted();
      if (gone > 0) {
        toast.warning(`${gone} deleted, ${failed.length} could not be.`, { duration: 20000 });
      }
    },
    onError: (e) => toast.error(e.message),
  });

  if (failures) {
    return (
      <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-lg flex-col gap-4 p-5">
        <h2 className="shrink-0 text-sm font-semibold text-text">
          {failures.length} could not be deleted
        </h2>
        <p className="shrink-0 text-xs text-muted">
          The rest were moved to the recycle bin. These were left exactly as they were.
        </p>
        <ul className="min-h-0 flex-1 space-y-1 overflow-auto text-xs">
          {failures.map((f) => (
            <li key={f.id} className="rounded border border-border px-2 py-1.5">
              <span className="id-mono text-faint">#{f.id}</span>
              <span className="ml-2 text-danger">{f.error}</span>
            </li>
          ))}
        </ul>
        <div className="flex shrink-0 justify-end">
          <Button size="sm" onClick={onClose}>
            Close
          </Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-lg flex-col gap-4 p-5">
      <div className="shrink-0">
        <h2 className="text-sm font-semibold text-text">
          Delete {cases.length} test case{cases.length === 1 ? "" : "s"}?
        </h2>
        <p className="mt-1 text-xs text-muted">
          These move to the recycle bin in Azure DevOps, where an administrator can restore them.
          This app cannot put them back, and it never deletes anything permanently.
        </p>
      </div>

      <ul className="min-h-0 flex-1 space-y-1 overflow-auto rounded-md border border-border p-2 text-xs">
        {cases.map((c) => (
          <li key={c.id} className="flex gap-2 px-1 py-0.5">
            <span className="id-mono shrink-0 text-faint">#{c.id}</span>
            <span className="text-text">{c.title}</span>
          </li>
        ))}
      </ul>

      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={remove.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          variant="danger"
          disabled={remove.isPending || cases.length === 0}
          onClick={() => remove.mutate()}
        >
          <IconRemove aria-hidden />
          {remove.isPending ? "Deleting" : `Delete ${cases.length}`}
        </Button>
      </div>
    </Modal>
  );
}
