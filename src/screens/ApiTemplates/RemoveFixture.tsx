import { useMutation } from "@tanstack/react-query";
import { useId } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

/**
 * The confirmation for removing a fixture, as RemoveTemplate's is for a
 * template: a plain confirm that names it. Removing takes the fixture and
 * its run history off this machine. What its runs made stays in the record
 * of test-made drafts, so Clean up can still find it; a script that names
 * the fixture is Blocked until the fixture is saved again.
 */
export default function RemoveFixture({
  org,
  project,
  fixture,
  onClose,
  onRemoved,
}: {
  org: string;
  project: string;
  fixture: { id: string; name: string };
  onClose: () => void;
  onRemoved: () => void;
}) {
  const headingId = useId();
  const remove = useMutation({
    mutationFn: () => unwrapStr(commands.apiFixtureRemove(org, project, fixture.id)),
    onSuccess: () => {
      toast.success(`Removed ${fixture.name}.`);
      onRemoved();
      onClose();
    },
    onError: (e) => toast.error(e.message),
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex w-full max-w-md flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Remove {fixture.name}?
      </h2>
      <p className="text-xs text-muted">
        It is removed from this machine with its run history. What it already made stays in the
        application and in the record of test-made drafts. A script that uses it cannot run until it is
        saved again. There is no undo.
      </p>
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" disabled={remove.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Keep
        </Button>
        <Button size="sm" variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>
          <IconRemove aria-hidden />
          Remove
        </Button>
      </div>
    </Modal>
  );
}
