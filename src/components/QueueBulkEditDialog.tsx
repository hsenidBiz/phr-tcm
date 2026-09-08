import { useState } from "react";
import type { TestCase } from "../bindings";
import ModuleField from "./ModuleField";
import TagsField from "./TagsField";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Textarea } from "./ui/input";
import { Modal } from "./ui/modal";
import { Select } from "./ui/select";
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
  const [status, setStatus] = useState(""); // "" = unchanged
  const [moduleValue, setModuleValue] = useState("");
  const [applyModule, setApplyModule] = useState(false);
  const [tagsMode, setTagsMode] = useState<"unchanged" | "add" | "replace">("unchanged");
  const [tags, setTags] = useState("");
  const [preconditions, setPreconditions] = useState("");
  const [applyPreconditions, setApplyPreconditions] = useState(false);

  const nothingChosen =
    !status && !applyModule && tagsMode === "unchanged" && !applyPreconditions;

  const buildEdit = (): ((tc: TestCase) => TestCase) => {
    // Captured once, so the edit is the same pure function for every case.
    const chosen = { status, moduleValue, applyModule, tagsMode, tags, preconditions, applyPreconditions };
    return (tc) => {
      const mergedTags =
        chosen.tagsMode === "replace"
          ? chosen.tags.trim()
          : chosen.tagsMode === "add"
            ? [
                ...tc.tags.split(";").map((t) => t.trim()).filter(Boolean),
                ...chosen.tags.split(";").map((t) => t.trim()).filter(Boolean),
              ]
                .filter((t, idx, arr) => arr.indexOf(t) === idx)
                .join("; ")
            : tc.tags;
      return {
        ...tc,
        tags: mergedTags,
        automation_status: chosen.status || tc.automation_status,
        module_value: chosen.applyModule ? chosen.moduleValue : tc.module_value,
        preconditions: chosen.applyPreconditions ? chosen.preconditions : tc.preconditions,
      };
    };
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
      </div>

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
