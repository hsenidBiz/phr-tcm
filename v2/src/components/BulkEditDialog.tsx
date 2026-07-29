import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type TestCase, type TestCaseFull } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import ModuleField from "./ModuleField";
import TagsField from "./TagsField";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Textarea } from "./ui/input";
import { Modal } from "./ui/modal";
import { Select } from "./ui/select";
import { IconCancel, IconConfirm } from "../lib/actionIcons";

/** Bulk edit for the selected cases: every field defaults to "leave
 * unchanged"; chosen fields are applied to each case IN ADDITION to its
 * existing content (steps are always preserved - each update sends the
 * case's own steps back). Applied serially with progress. */
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
  const [status, setStatus] = useState(""); // "" = unchanged
  const [moduleValue, setModuleValue] = useState("");
  const [applyModule, setApplyModule] = useState(false);
  const [tagsMode, setTagsMode] = useState<"unchanged" | "add" | "replace">("unchanged");
  const [tags, setTags] = useState("");
  const [preconditions, setPreconditions] = useState("");
  const [applyPreconditions, setApplyPreconditions] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  const nothingChosen =
    !status && !applyModule && tagsMode === "unchanged" && !applyPreconditions;

  const apply = useMutation({
    mutationFn: async () => {
      let failed = 0;
      for (let i = 0; i < cases.length; i++) {
        setProgress({ done: i + 1, total: cases.length });
        const c = cases[i];
        const mergedTags =
          tagsMode === "replace"
            ? tags.trim()
            : tagsMode === "add"
              ? [
                  ...c.tags.split(";").map((t) => t.trim()).filter(Boolean),
                  ...tags.split(";").map((t) => t.trim()).filter(Boolean),
                ]
                  .filter((t, idx, arr) => arr.indexOf(t) === idx)
                  .join("; ")
              : c.tags;
        const tc: TestCase = {
          title: c.title,
          // Handed straight back unchanged, so the save compares them equal
          // to the original and omits Steps from the patch altogether.
          steps: c.steps,
          tags: mergedTags,
          automation_status: status || c.automation_status,
          module_value: applyModule ? moduleValue : c.module_value,
          preconditions: applyPreconditions ? preconditions : c.preconditions,
          update_id: c.id,
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
        );
        if (r.status === "error") failed++;
      }
      return failed;
    },
    onSettled: () => setProgress(null),
    onSuccess: (failed) => {
      if (failed === 0) toast.success(`Updated ${cases.length} test case(s).`);
      else toast.warning(`${cases.length - failed} updated, ${failed} failed.`);
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
        <label className="block text-xs text-muted">
          Automation status
          <Select className="mt-1 w-full" value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="">Leave unchanged</option>
            <option>Not Automated</option>
            <option>Planned</option>
          </Select>
        </label>

        <div className="space-y-1">
          <label className="flex items-center gap-2 text-xs text-muted">
            <Checkbox checked={applyModule} onCheckedChange={setApplyModule} />
            Set module
          </label>
          {applyModule && (
            <ModuleField
              org={org}
              project={project}
              value={moduleValue}
              onChange={setModuleValue}
              className="w-full"
            />
          )}
        </div>

        <div className="space-y-1">
          <label className="block text-xs text-muted">
            Tags
            <Select
              className="mt-1 w-full"
              value={tagsMode}
              onChange={(e) => setTagsMode(e.target.value as typeof tagsMode)}
            >
              <option value="unchanged">Leave unchanged</option>
              <option value="add">Add tags</option>
              <option value="replace">Replace tags</option>
            </Select>
          </label>
          {tagsMode !== "unchanged" && (
            <TagsField
              org={org}
              project={project}
              className="w-full"
              value={tags}
              onChange={setTags}
              placeholder={tagsMode === "add" ? "Tags to add…" : "Replacement tags…"}
            />
          )}
        </div>

        <div className="space-y-1">
          <label className="flex items-center gap-2 text-xs text-muted">
            <Checkbox checked={applyPreconditions} onCheckedChange={setApplyPreconditions} />
            Set preconditions
          </label>
          {applyPreconditions && (
            <Textarea
              className="h-16 w-full"
              placeholder="Preconditions"
              value={preconditions}
              onChange={(e) => setPreconditions(e.target.value)}
            />
          )}
        </div>

        {progress && (
          <p className="text-xs text-muted">
            Updating {progress.done} of {progress.total}
          </p>
        )}
      </div>

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
