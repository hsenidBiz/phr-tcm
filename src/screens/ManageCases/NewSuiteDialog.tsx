import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconNewSuite } from "../../lib/actionIcons";
import { unwrap } from "../../lib/ipc";

/** A static test suite, created under a static suite or the plan root (the
 * only parents Azure DevOps allows). With cases selected the same click
 * copies them in afterwards: they stay where they were, a case can live
 * in many suites. */
export default function NewSuiteDialog({
  org,
  project,
  planId,
  parents,
  defaultParentId,
  caseIds,
  sourceLabel,
  onClose,
  onCreated,
}: {
  org: string;
  project: string;
  planId: number;
  parents: Array<{ id: number; label: string }>;
  defaultParentId: number;
  caseIds: number[];
  /** A sentence fragment for the "they stay in X too" line, e.g. "their suites". */
  sourceLabel: string;
  onClose: () => void;
  onCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [parentId, setParentId] = useState(String(defaultParentId));
  const trimmed = name.trim();
  const n = caseIds.length;

  const create = useMutation({
    // The suite and the copy are two separate ADO calls: once the suite
    // exists, it's real and listed whether or not the copy that follows
    // succeeds. So a failed copy is not a failed creation - it's reported
    // with its own toast, but still closes the dialog and refreshes the
    // tree so the (now empty) suite shows up.
    mutationFn: async () => {
      const suite = await unwrap(commands.createStaticSuite(org, project, planId, Number(parentId), trimmed));
      if (n === 0) return { suite, added: null as number[] | null, addError: null as string | null };
      try {
        const added = await unwrap(commands.addCasesToSuite(org, project, planId, suite.id, caseIds));
        return { suite, added, addError: null as string | null };
      } catch (e) {
        return { suite, added: null as number[] | null, addError: e instanceof Error ? e.message : String(e) };
      }
    },
    onSuccess: ({ suite, added, addError }) => {
      if (addError) {
        toast.warning(`Created suite "${suite.name}", but the test cases could not be added: ${addError}`, {
          duration: 20000,
        });
      } else if (n === 0) {
        toast.success(`Created suite "${suite.name}".`);
      } else {
        toast.success(
          `Created suite "${suite.name}" and added ${added!.length} test case${added!.length === 1 ? "" : "s"}. They stay in ${sourceLabel} too.`,
        );
      }
      onCreated();
      onClose();
    },
    onError: (e) => toast.error(`Could not create the suite: ${e.message}`),
  });

  return (
    <Modal onClose={onClose} className="w-full max-w-md space-y-3 p-5">
      <h2 className="text-sm font-semibold text-text">New test suite</h2>
      <p className="text-xs text-muted">
        A static test suite. It can sit under the plan root or under another static suite.
      </p>
      <label className="block text-xs text-muted">
        Suite name
        <Input
          aria-label="Suite name"
          autoFocus
          className="mt-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Smoke"
        />
      </label>
      <label className="block text-xs text-muted">
        Create inside
        <Select aria-label="Create inside" triggerClassName="mt-1" value={parentId} onChange={(e) => setParentId(e.target.value)}>
          {parents.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </Select>
      </label>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={create.isPending} onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={!trimmed || create.isPending} onClick={() => create.mutate()}>
          <IconNewSuite aria-hidden />
          {create.isPending
            ? "Creating"
            : n > 0
              ? `Create suite and add ${n} test case${n === 1 ? "" : "s"}`
              : "Create suite"}
        </Button>
      </div>
    </Modal>
  );
}
