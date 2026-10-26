import { IconClear } from "../lib/actionIcons";
import ModuleField from "./ModuleField";
import PrioritySelect from "./PrioritySelect";
import TagsField from "./TagsField";
import { Button } from "./ui/button";
import { Textarea } from "./ui/input";
import { Select } from "./ui/select";

/** The fields both bulk edit dialogs offer (the work items' one, the
 * queue's one). Nothing is switched on to be changed: every field shows at
 * once and starts at "Leave unchanged", and a field counts as set when it
 * holds something other than that. */
export type BulkValues = {
  status: string; // "" = unchanged
  priority: number | null; // null = unchanged
  moduleValue: string; // "" (or only spaces) = unchanged
  moduleClear: boolean; // true = empty the module on every case
  tagsMode: "unchanged" | "add" | "replace";
  tags: string;
  preconditionsMode: "unchanged" | "replace" | "clear";
  preconditions: string;
};

export const NO_BULK_CHANGE: BulkValues = {
  status: "",
  priority: null,
  moduleValue: "",
  moduleClear: false,
  tagsMode: "unchanged",
  tags: "",
  preconditionsMode: "unchanged",
  preconditions: "",
};

/** Replace with nothing typed is not yet a change: emptying the text is what
 * Clear is for, and a stray empty box must not wipe preconditions. */
function preconditionsSet(v: BulkValues): boolean {
  return (
    v.preconditionsMode === "clear" ||
    (v.preconditionsMode === "replace" && v.preconditions.trim() !== "")
  );
}

/** A tag is typed: Add with nothing in the box would only re-format each
 * case's tags, so it is not a change. */
function tagsTyped(v: BulkValues): boolean {
  return v.tags.split(";").some((t) => t.trim() !== "");
}

/** The set fields' names, in the order the form shows them. */
export function bulkChangedFields(v: BulkValues): string[] {
  const out: string[] = [];
  if (v.status) out.push("Automation status");
  if (v.priority != null) out.push("Priority");
  if (v.moduleClear) out.push("Module (clear)");
  else if (v.moduleValue.trim() !== "") out.push("Module");
  if (v.tagsMode === "add" && tagsTyped(v)) out.push("Tags");
  // Replace with nothing typed is how tags are cleared, so it counts.
  if (v.tagsMode === "replace") out.push(v.tags.trim() === "" ? "Tags (clear)" : "Tags");
  if (preconditionsSet(v)) out.push("Preconditions");
  return out;
}

/** A case's module after the chosen change: empty when cleared, the
 * trimmed pick when one is set, the case's own otherwise. */
export function bulkModule(v: BulkValues, current: string): string {
  if (v.moduleClear) return "";
  return v.moduleValue.trim() || current;
}

/** A case's tags after the chosen tag change. */
export function bulkTags(v: BulkValues, current: string): string {
  if (v.tagsMode === "replace") return v.tags.trim();
  if (v.tagsMode === "add" && tagsTyped(v)) {
    const split = (s: string) => s.split(";").map((t) => t.trim()).filter(Boolean);
    return [...split(current), ...split(v.tags)]
      .filter((t, idx, arr) => arr.indexOf(t) === idx)
      .join("; ");
  }
  return current;
}

/** A case's preconditions after the chosen change. */
export function bulkPreconditions(v: BulkValues, current: string): string {
  if (v.preconditionsMode === "clear") return "";
  if (preconditionsSet(v)) return v.preconditions;
  return current;
}

export function BulkFields({
  org,
  project,
  value,
  onChange,
}: {
  org: string;
  project: string;
  value: BulkValues;
  onChange: (next: BulkValues) => void;
}) {
  const set = (patch: Partial<BulkValues>) => onChange({ ...value, ...patch });
  return (
    <>
      <label className="block text-xs text-muted">
        Automation status
        <Select
          className="mt-1 w-full"
          value={value.status}
          onChange={(e) => set({ status: e.target.value })}
        >
          <option value="">Leave unchanged</option>
          <option>Not Automated</option>
          <option>Planned</option>
        </Select>
      </label>

      <label className="block text-xs text-muted">
        Priority
        <PrioritySelect
          className="mt-1 w-full"
          value={value.priority}
          onChange={(priority) => set({ priority })}
          keepLabel="Leave unchanged"
        />
      </label>

      <div className="space-y-1">
        <span className="block text-xs text-muted">Module</span>
        <div className="flex items-center gap-2">
          <ModuleField
            org={org}
            project={project}
            value={value.moduleValue}
            // Picking a module lets go of Clear; emptying the picker does not.
            onChange={(moduleValue) =>
              set({ moduleValue, moduleClear: moduleValue.trim() !== "" ? false : value.moduleClear })
            }
            placeholder={value.moduleClear ? "No module (clears it)" : "Leave unchanged"}
            className="min-w-0 flex-1"
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-pressed={value.moduleClear}
            className={value.moduleClear ? "border-accent bg-accent-soft text-accent" : undefined}
            onClick={() => set({ moduleClear: !value.moduleClear, moduleValue: "" })}
          >
            <IconClear aria-hidden />
            Clear module
          </Button>
        </div>
      </div>

      <div className="space-y-1">
        <label className="block text-xs text-muted">
          Tags
          <Select
            className="mt-1 w-full"
            value={value.tagsMode}
            onChange={(e) => set({ tagsMode: e.target.value as BulkValues["tagsMode"] })}
          >
            <option value="unchanged">Leave unchanged</option>
            <option value="add">Add tags</option>
            <option value="replace">Replace tags</option>
          </Select>
        </label>
        {value.tagsMode !== "unchanged" && (
          <TagsField
            org={org}
            project={project}
            className="w-full"
            value={value.tags}
            onChange={(tags) => set({ tags })}
            placeholder={value.tagsMode === "add" ? "Tags to add…" : "Replacement tags…"}
          />
        )}
      </div>

      <div className="space-y-1">
        <label className="block text-xs text-muted">
          Preconditions
          <Select
            aria-label="Preconditions"
            className="mt-1 w-full"
            value={value.preconditionsMode}
            onChange={(e) =>
              set({ preconditionsMode: e.target.value as BulkValues["preconditionsMode"] })
            }
          >
            <option value="unchanged">Leave unchanged</option>
            <option value="replace">Replace with</option>
            <option value="clear">Clear</option>
          </Select>
        </label>
        {value.preconditionsMode === "replace" && (
          <Textarea
            aria-label="Preconditions text"
            className="h-16 w-full"
            placeholder="Preconditions"
            value={value.preconditions}
            onChange={(e) => set({ preconditions: e.target.value })}
          />
        )}
      </div>
    </>
  );
}

/** The muted line above the buttons: what Apply will change. */
export function BulkSummary({ value }: { value: BulkValues }) {
  const changed = bulkChangedFields(value);
  return (
    <p className="shrink-0 text-xs text-muted">
      {changed.length > 0 ? `Will change: ${changed.join(", ")}` : "Nothing to change yet"}
    </p>
  );
}
