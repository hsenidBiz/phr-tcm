import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type DeleteOutcome } from "../bindings";
import { describeAdoError, unwrap } from "../lib/ipc";
import { IconCancel, IconRemove } from "../lib/actionIcons";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Modal } from "./ui/modal";

/**
 * The confirmation for the one action this app cannot undo from inside.
 *
 * Everything else it does is a create or an update, and a mistake can be
 * edited back. A delete cannot - once gone, nothing brings a test case
 * back. So the whole list is shown by id and title
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
  const [acknowledged, setAcknowledged] = useState(false);
  /** How many actually went, so the panel does not claim a "rest" that
   * does not exist when every one of them failed. */
  const [deleted, setDeleted] = useState(0);

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
      setDeleted(gone);
      // Only when something actually went. onDeleted refreshes the list AND
      // clears the selection, so calling it when every delete failed threw
      // away the selection of cases that all still exist - leaving the user
      // to re-find and re-select them to try again.
      if (gone > 0) {
        onDeleted();
        toast.warning(`${gone} deleted, ${failed.length} could not be.`, { duration: 20000 });
      } else {
        toast.error(`None of the ${failed.length} could be deleted.`, { duration: 20000 });
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
          {deleted > 0
            ? `The other ${deleted} were permanently deleted. These were left exactly as they were.`
            : "Nothing was deleted - every one of these was left exactly as it was."}
        </p>
        <ul className="min-h-0 flex-1 space-y-1 overflow-auto text-xs">
          {failures.map((f) => (
            <li key={f.id} className="rounded border border-border px-2 py-1.5">
              <span className="id-mono text-faint">#{f.id}</span>
              {/* describeAdoError lifts Azure DevOps' own `message` out of
                  the response body. Before this the Rust side flattened
                  the error to a string first, so an unmapped status
                  arrived here as the literal text "http 400" and the
                  explanation ADO had sent was never shown to anyone. */}
              <span className="ml-2 text-danger">
                {f.error ? describeAdoError(f.error) : "Unknown error."}
              </span>
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
        {/* The plain truth, plainly. Azure DevOps offers NO recoverable
            deletion for test artifacts - the work-item recycle bin refuses
            them outright - so this goes through the Test Management API
            and is final. The wording must never soften: a person deciding
            to destroy twelve named cases is owed the word "permanent". */}
        <p className="mt-1 text-xs text-danger">
          This is permanent. Azure DevOps deletes test cases through the Test Management
          API with no recycle bin behind it - these cases and their run history cannot be
          restored by this app, an administrator, or anyone else.
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

      {/* The button stays dead until the finality is acknowledged - for
          an action with no undo, one deliberate extra click is the whole
          difference between a decision and a slip. */}
      <label className="flex shrink-0 cursor-pointer items-center gap-2 text-xs text-muted">
        <Checkbox
          checked={acknowledged}
          ariaLabel="I understand these test cases will be permanently deleted"
          onCheckedChange={setAcknowledged}
        />
        I understand {cases.length === 1 ? "this test case" : `these ${cases.length} test cases`} will
        be permanently deleted and cannot be restored.
      </label>

      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={remove.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          variant="danger"
          disabled={remove.isPending || cases.length === 0 || !acknowledged}
          onClick={() => remove.mutate()}
        >
          <IconRemove aria-hidden />
          {remove.isPending ? "Deleting" : `Permanently delete ${cases.length}`}
        </Button>
      </div>
    </Modal>
  );
}
