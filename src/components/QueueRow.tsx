import { ChevronDown, ChevronRight, MessageSquare } from "lucide-react";
import { memo } from "react";
import type { TestCase } from "../bindings";
import { diffSummary, type CaseDiff } from "../lib/caseDiff";
import { cn } from "../lib/cn";
import CaseStepsTable from "./CaseStepsTable";
import InlineDiff from "./InlineDiff";
import QueueCaseEditor from "./QueueCaseEditor";
import StepDiffLines from "./StepDiffLines";
import { Badge } from "./ui/badge";
import { Checkbox } from "./ui/checkbox";

export type QueueRowProps = {
  tc: TestCase;
  index: number;
  org: string;
  project: string;
  isSelected: boolean;
  stepsOpen: boolean;
  diffOpen: boolean;
  editing: boolean;
  /** This row failed the last submit and still needs a decision. */
  failed: boolean;
  /** A watched-file sync just added or changed this row. */
  touched: "added" | "changed" | undefined;
  reviewing: boolean;
  /** Validation problem, shown in review. */
  problem: string | null;
  /** Duplicate-title warning, shown in review when there is no problem. */
  duplicate: string | null;
  diff: CaseDiff | null;
  diffFailed: boolean;
  /** A submit is running - Edit and Remove are disabled meanwhile. */
  busy: boolean;
  onToggleSelect: (index: number, shift: boolean) => void;
  onToggleSteps: (index: number) => void;
  onToggleDiff: (index: number) => void;
  onToggleEdit: (index: number) => void;
  onRemove: (index: number) => void;
  onSave: (index: number, next: TestCase) => void;
  onCancelEdit: () => void;
};

/** One queued case. Pure - no hooks - so the memoised export below is the
 * whole story: a row re-renders only when one of ITS props changes.
 *
 * That is what keeps the queue usable at a hundred cases. QueueSection
 * re-renders several times on every mount (its own effects, the
 * on-screen observer, the existing-cases query) and on every selection or
 * progress tick, and each of those used to rebuild every row. With the
 * callbacks stable and the per-row values primitives, those renders now
 * touch only the rows that actually changed. */
export function QueueRowInner({
  tc,
  index: i,
  org,
  project,
  isSelected,
  stepsOpen,
  diffOpen,
  editing,
  failed,
  touched,
  reviewing,
  problem,
  duplicate,
  diff,
  diffFailed,
  busy,
  onToggleSelect,
  onToggleSteps,
  onToggleDiff,
  onToggleEdit,
  onRemove,
  onSave,
  onCancelEdit,
}: QueueRowProps) {
  return (
    <li
      className={cn(
        // The open editor hosts a non-portaled Combobox dropdown that must
        // paint past the row's box - content-visibility's paint containment
        // would clip it, so drop cv-row while this row is being edited.
        !editing && "cv-row",
        "rounded-md border text-sm transition-colors",
        // Ranked above the file-sync colours on purpose: a row that
        // failed to upload needs a decision now, and that outranks
        // where its text last came from.
        failed
          ? "border-danger/60 bg-danger/5"
          : touched === "added"
            ? "border-success/50 bg-success/5"
            : touched === "changed"
              ? "border-warning/50 bg-warning/5"
              : "border-border",
      )}
    >
      <div className="flex items-center justify-between px-3 py-1.5">
        <span className="text-text">
          {/* Capture-phase wrapper: the checkbox's own click never
              fires, so shift-ranges can be read off the event. */}
          <span
            className="mr-2 inline-block align-middle"
            onClickCapture={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onToggleSelect(i, e.shiftKey);
            }}
          >
            <Checkbox ariaLabel={`Select ${tc.title}`} checked={isSelected} onCheckedChange={() => {}} />
          </span>
          <button
            aria-label={stepsOpen ? `Collapse steps of ${tc.title}` : `Expand steps of ${tc.title}`}
            title={stepsOpen ? "Hide steps" : "Check the steps before submitting"}
            className="mr-2 align-middle text-muted hover:text-accent"
            onClick={() => onToggleSteps(i)}
          >
            {stepsOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
          {tc.update_id != null ? (
            <Badge className="mr-2 bg-warning/20 text-warning">UPDATE #{tc.update_id}</Badge>
          ) : (
            <Badge className="mr-2 bg-success/20 text-success">NEW</Badge>
          )}
          {tc.title}
          <span className="ml-2 text-xs text-faint">{tc.steps.length} steps</span>
          {diff?.noop && (
            <Badge className="ml-2 bg-warning/20 text-warning">no-op — nothing will change</Badge>
          )}
          {diff && !diff.noop && (
            <button className="ml-2 text-xs text-accent hover:underline" onClick={() => onToggleDiff(i)}>
              {diffSummary(diff)} {diffOpen ? "▾" : "▸"}
            </button>
          )}
          {diffFailed && <span className="ml-2 text-xs text-faint">diff unavailable</span>}
          {reviewing && problem && <span className="ml-2 text-xs text-danger">{problem}</span>}
          {reviewing && !problem && duplicate && (
            <span className="ml-2 text-xs text-warning">{duplicate}</span>
          )}
        </span>
        <span className="flex items-center gap-3">
          <button
            className="text-xs text-faint hover:text-accent disabled:opacity-50"
            disabled={busy}
            onClick={() => onToggleEdit(i)}
          >
            {editing ? "Close" : "Edit"}
          </button>
          <button
            className="text-xs text-faint hover:text-danger disabled:opacity-50"
            disabled={busy}
            onClick={() => onRemove(i)}
          >
            Remove
          </button>
        </span>
      </div>
      {/* In-app note from the JSON file - never sent to ADO. */}
      {(tc.comment ?? "").trim() !== "" && (
        <p className="flex items-start gap-1.5 border-t border-border/60 px-3 py-1 text-xs text-muted">
          <MessageSquare size={12} className="mt-0.5 shrink-0" />
          <span className="min-w-0 flex-1 whitespace-pre-wrap">{tc.comment}</span>
        </p>
      )}
      {editing && (
        <QueueCaseEditor
          original={tc}
          org={org}
          project={project}
          onSave={(next) => onSave(i, next)}
          onCancel={onCancelEdit}
        />
      )}
      {stepsOpen && (
        <div className="border-t border-border">
          {/* Shared with the watched-file change report, which
              needed the same "read the case start to finish"
              view - see CaseStepsTable. */}
          <CaseStepsTable steps={tc.steps} preconditions={tc.preconditions} reviewerNotes={tc.reviewer_notes} />
        </div>
      )}
      {diff && !diff.noop && diffOpen && (
        <div className="space-y-1 border-t border-border px-3 py-2 text-xs">
          {/* Word-level, like the step lines below: editing one
              word of a title must not read as the whole title
              being replaced. */}
          {diff.fields.map((f) => (
            <div key={f.name}>
              <span className="font-medium text-muted">{f.name}:</span> <InlineDiff old={f.old} next={f.new} />
            </div>
          ))}
          {diff.steps.detail.length > 0 && (
            <div className="space-y-1">
              <span className="font-medium text-muted">Steps:</span>
              {/* git word-diff style: -/+ lines with only the
                  actually-changed words highlighted. */}
              {diff.steps.detail.map((d) => (
                <StepDiffLines key={d.index} d={d} />
              ))}
            </div>
          )}
          {diff.blankSkipped.length > 0 && (
            <div className="text-faint">Left untouched (blank in import): {diff.blankSkipped.join(", ")}</div>
          )}
        </div>
      )}
    </li>
  );
}

const QueueRow = memo(QueueRowInner);
export default QueueRow;
