import { useMutation } from "@tanstack/react-query";
import { useId } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

/** What becomes of the templates on the flow's stages: they stay, and are
 * refused until a flow with their stage is saved again (spec §8). */
function templatesSentence(n: number): string {
  if (n === 0) return "No template performs its stages.";
  if (n === 1) return "1 template performs its stages; it stays, and is refused until a flow with its stage is saved again.";
  return `${n} templates perform its stages; they stay, and are refused until a flow with their stage is saved again.`;
}

/**
 * The confirmation for removing a flow, as RemoveTemplate is for a
 * template: it names the flow and says what happens to the templates that
 * perform its stages. Only the flow's file goes; the assistant can save it
 * again. Removing emits no change event, so `onRemoved` must reload.
 */
export default function RemoveFlow({
  org,
  project,
  flow,
  templates,
  onClose,
  onRemoved,
}: {
  org: string;
  project: string;
  flow: { id: string; title: string };
  /** How many saved templates name this flow in their `stage`. */
  templates: number;
  onClose: () => void;
  onRemoved: () => void;
}) {
  const headingId = useId();
  const remove = useMutation({
    mutationFn: () => unwrapStr(commands.apiTemplatesRemoveFlow(org, project, flow.id)),
    onSuccess: () => {
      toast.success(`Removed ${flow.title}.`);
      onRemoved();
      onClose();
    },
    onError: (e) => toast.error(e.message),
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex w-full max-w-md flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Remove {flow.title}?
      </h2>
      <p className="text-xs text-muted">
        {templatesSentence(templates)} The flow is removed from this machine. There is no undo; the assistant
        can save it again.
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
