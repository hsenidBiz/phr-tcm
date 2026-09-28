import { useMutation } from "@tanstack/react-query";
import { useId } from "react";
import { commands, type Effect } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

/** What running the template does to the application's data, in the
 * dialog's own words. */
const EFFECT_SENTENCE: Record<Effect, string> = {
  create: "This template creates data.",
  edit: "This template edits data.",
  delete: "This template deletes data.",
};

/**
 * The confirmation for the tab's one action. It names the template and what
 * it does, because a person deciding to drop "Create a draft performance
 * cycle" should see that name, not a count. Removing takes the template's
 * file and its run history off this machine; the assistant can prove it
 * again, which is why this is a plain confirm and not DeleteConfirm's
 * acknowledged one - nothing in Azure DevOps or the application is touched.
 */
export default function RemoveTemplate({
  org,
  project,
  template,
  onClose,
  onRemoved,
}: {
  org: string;
  project: string;
  template: { id: string; title: string; effect: Effect };
  onClose: () => void;
  onRemoved: () => void;
}) {
  const headingId = useId();
  const remove = useMutation({
    mutationFn: () => unwrapStr(commands.apiTemplatesRemove(org, project, template.id)),
    onSuccess: () => {
      toast.success(`Removed ${template.title}.`);
      onRemoved();
      onClose();
    },
    onError: (e) => toast.error(e.message),
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex w-full max-w-md flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Remove {template.title}?
      </h2>
      <p className="text-xs text-muted">
        {EFFECT_SENTENCE[template.effect]} It is removed from this machine with its run history. There is
        no undo; the assistant can prove it again.
      </p>
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" disabled={remove.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Keep it
        </Button>
        <Button size="sm" variant="danger" disabled={remove.isPending} onClick={() => remove.mutate()}>
          <IconRemove aria-hidden />
          Remove
        </Button>
      </div>
    </Modal>
  );
}
