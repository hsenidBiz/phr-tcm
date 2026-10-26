import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "../lib/toast";
import { commands, type TestCase, type TestCaseFull } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import {
  BulkFields,
  BulkSummary,
  NO_BULK_CHANGE,
  bulkChangedFields,
  bulkModule,
  bulkPreconditions,
  bulkTags,
} from "./BulkFields";
import { Button } from "./ui/button";
import { Modal } from "./ui/modal";
import { IconCancel, IconConfirm } from "../lib/actionIcons";

/** Bulk edit for the selected cases: every field shows at once and defaults
 * to "leave unchanged"; chosen fields are applied to each case IN ADDITION
 * to its existing content (steps are always preserved - each update sends
 * the case's own steps back). Applied serially with progress. */
export default function BulkEditDialog({
  org,
  project,
  cases,
  onClose,
  onDone,
}: {
  org: string;
  project: string;
  cases: TestCaseFull[];
  onClose: () => void;
  onDone: () => void;
}) {
  const { prefs } = useFieldRefs(org, project);
  const [values, setValues] = useState(NO_BULK_CHANGE);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  const nothingChosen = bulkChangedFields(values).length === 0;

  const apply = useMutation({
    mutationFn: async () => {
      const failed: { title: string; error: string }[] = [];
      for (let i = 0; i < cases.length; i++) {
        setProgress({ done: i + 1, total: cases.length });
        const c = cases[i];
        const tc: TestCase = {
          title: c.title,
          // Handed straight back unchanged, so the save compares them equal
          // to the original and omits Steps from the patch altogether.
          steps: c.steps,
          tags: bulkTags(values, c.tags),
          automation_status: values.status || c.automation_status,
          module_value: bulkModule(values, c.module_value),
          preconditions: bulkPreconditions(values, c.preconditions),
          update_id: c.id,
          // Null leaves each work item's own priority alone.
          priority: values.priority,
        };
        const r = await commands.updateTestCase(
          org,
          project,
          tc,
          prefs.moduleRef,
          prefs.preconditionsRef,
          // What makes "steps are never touched" below actually true: with
          // the original XML in hand the save leaves Steps out of the patch
          // entirely, instead of rewriting them from a plain-text read.
          c.steps_xml,
          // The current tags decide which PATCH op can actually REMOVE a
          // tag - Azure DevOps merges plain tag writes.
          c.tags,
          // Likewise the prerequisites: left alone unless this edit sets them.
          c.preconditions_stored ?? null,
        );
        // The message is the actionable half. Counting the failures and
        // dropping WHY told the user "2 failed" about a selection of
        // twenty, with no way to tell which two or what to change.
        if (r.status === "error") failed.push({ title: c.title, error: r.error });
      }
      return failed;
    },
    onSettled: () => setProgress(null),
    onSuccess: (failed) => {
      if (failed.length === 0) {
        toast.success(`Updated ${cases.length} test case(s).`);
      } else {
        // Named, with the reason. Several cases usually fail the same way
        // (one field rule), so identical reasons are grouped rather than
        // repeated once per case.
        const byReason = new Map<string, string[]>();
        for (const f of failed) {
          byReason.set(f.error, [...(byReason.get(f.error) ?? []), f.title]);
        }
        const detail = [...byReason]
          .map(([reason, titles]) => `${titles.join(", ")}: ${reason}`)
          .join(" | ");
        toast.warning(
          `${cases.length - failed.length} updated, ${failed.length} failed - ${detail}`,
          { duration: 20000 },
        );
      }
      onDone();
    },
    onError: (e) => toast.error(`Bulk edit failed: ${e.message}`),
  });

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-md flex-col gap-4 p-5">
      <div className="shrink-0">
        <h2 className="text-sm font-semibold text-text">
          Bulk edit {cases.length} test case{cases.length === 1 ? "" : "s"}
        </h2>
        <p className="text-xs text-muted">
          Only the fields you set here change - titles and steps are never touched.
        </p>
      </div>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
        <BulkFields org={org} project={project} value={values} onChange={setValues} />

        {progress && (
          <p className="text-xs text-muted">
            Updating {progress.done} of {progress.total}
          </p>
        )}
      </div>

      <BulkSummary value={values} />

      <div className="flex shrink-0 justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={nothingChosen || apply.isPending} onClick={() => apply.mutate()}>
          <IconConfirm aria-hidden />
          {apply.isPending ? "Applying" : `Apply to ${cases.length}`}
        </Button>
      </div>
    </Modal>
  );
}
