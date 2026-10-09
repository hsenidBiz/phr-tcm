import { ChevronDown, ChevronRight, MessageSquare } from "lucide-react";
import { memo, type KeyboardEvent, type MouseEvent } from "react";
import type { TestCase } from "../bindings";
import { diffSummary, retypedLines, type CaseDiff } from "../lib/caseDiff";
import { cn } from "../lib/cn";
import { fileChangeSummary, type SyncChange } from "../lib/fileSync";
import CaseStepsTable from "./CaseStepsTable";
import QueueCaseEditor from "./QueueCaseEditor";
import CaseChangeDetail from "./CaseChangeDetail";
import { Badge } from "./ui/badge";
import { Collapse } from "./ui/collapse";

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
  /** The last submit wrote this row to Azure DevOps. It stays queued until
   * the user removes it, so this is what tells it apart from a row still
   * waiting to go. */
  uploaded: boolean;
  /** The last upload could not confirm whether this create landed
   * (lib/uploadHold). It may exist - Upload is held until a Check. */
  held: boolean;
  /** Held specifically because Azure DevOps has more than one test case
   * with this title (lib/uploadHold's `ambiguous`), not merely because the
   * outcome is unknown - a Check cannot tell which one is this row's, so
   * the row says that instead of the generic held message. Deviation from
   * the brief: see uploadHold.ts. */
  ambiguous: boolean;
  /** A watched-file sync just added or changed this row. */
  touched: "added" | "changed" | undefined;
  /** What an open change report says the watched file did to this NEW row,
   * when it CHANGED it. Absent for an added row, an UPDATE row, and once
   * the report is dismissed. Opens with the same toggle as `diff`. */
  fileChange?: SyncChange;
  reviewing: boolean;
  /** Validation problem, shown in review. */
  problem: string | null;
  /** Duplicate-title warning, shown in review when there is no problem. */
  duplicate: string | null;
  diff: CaseDiff | null;
  diffFailed: boolean;
  /** A submit is running - Edit and Remove are disabled meanwhile. */
  busy: boolean;
  /** Select this row the way Update Test Cases does: `range` (Shift)
   * takes the cases from the anchor to this one, `toggle` (Ctrl/Cmd) adds
   * or removes just this one or, with `range`, adds the range. */
  onToggleSelect: (index: number, how: { range: boolean; toggle: boolean }) => void;
  /** Base for this row's element ids (the section's useId), so its name can
   * point at its own title and badges. */
  idBase: string;
  /** The one row of the grid in the Tab order (a roving tabindex): the rest
   * are reached with the arrow keys. */
  tabStop: boolean;
  /** An arrow key, Home or End on the row: move to another row, Shift to
   * extend the selection on the way. */
  onNavigate: (index: number, key: "ArrowUp" | "ArrowDown" | "Home" | "End", extend: boolean) => void;
  /** Focus arrived in this row (on it or on one of its buttons). */
  onRowFocus: (index: number) => void;
  onToggleSteps: (index: number) => void;
  onToggleDiff: (index: number) => void;
  onToggleEdit: (index: number) => void;
  onRemove: (index: number) => void;
  onSave: (index: number, next: TestCase) => void;
  onCancelEdit: () => void;
};

/** What inside a row keeps a click for itself: its buttons and links, any
 * field, and the parts marked `data-row-ignore` (the open editor and the
 * steps and change panels, which are there to be read and copied). */
const OWN_CLICKS =
  "button, a, input, textarea, select, label, [role='button'], [role='combobox'], [role='listbox'], [role='option'], [role='switch'], [role='checkbox'], [contenteditable='true'], [data-row-ignore]";

/** True when the event is aimed at the row itself, not at something in it
 * with a job of its own. A click that bubbled out of a portal (a dropdown
 * or dialog the editor opened) is not inside the row's DOM at all. */
function onRowItself(e: MouseEvent<HTMLElement> | KeyboardEvent<HTMLElement>): boolean {
  const row = e.currentTarget;
  const target = e.target as Element;
  if (!(target instanceof Element) || !row.contains(target)) return false;
  const own = target.closest(OWN_CLICKS);
  return !own || !row.contains(own);
}

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
  uploaded,
  held,
  ambiguous,
  touched,
  fileChange,
  reviewing,
  problem,
  duplicate,
  diff,
  diffFailed,
  busy,
  onToggleSelect,
  idBase,
  tabStop,
  onNavigate,
  onRowFocus,
  onToggleSteps,
  onToggleDiff,
  onToggleEdit,
  onRemove,
  onSave,
  onCancelEdit,
}: QueueRowProps) {
  // A NEW row only: an UPDATE row keeps its Azure DevOps diff and nothing else.
  const showFileDiff = fileChange != null && tc.update_id == null;
  // The row's name is its title and then its status - what the badges and
  // warnings say - so a screen reader hears more than the title alone.
  const id = (part: string) => `${idBase}-${i}-${part}`;
  const status = [
    "title",
    "op",
    uploaded && "uploaded",
    held && "held",
    failed && "failed",
    diff?.noop && "noop",
    reviewing && (problem || duplicate) && "review",
  ].filter((part): part is string => Boolean(part));
  return (
    // Selected by clicking the row itself, as on Update Test Cases: a row
    // (a div: a list item cannot take the role) of a grid that allows
    // several selected rows. One row is in the Tab order; the arrow keys
    // move between rows, and Space or Enter does what a click does. Its
    // buttons, links and fields keep their clicks (OWN_CLICKS).
    <div
      role="row"
      id={id("row")}
      aria-selected={isSelected}
      aria-labelledby={status.map(id).join(" ")}
      tabIndex={tabStop ? 0 : -1}
      onFocus={() => onRowFocus(i)}
      onClick={(e) => {
        if (!onRowItself(e)) return;
        // A double-click to pick a word, or a drag across the text, is
        // reading or copying - not choosing cases.
        if (e.detail > 1) return;
        const text = window.getSelection();
        if (text && !text.isCollapsed) return;
        onToggleSelect(i, { range: e.shiftKey, toggle: e.ctrlKey || e.metaKey });
      }}
      // A Shift-click would otherwise also drag a text selection across
      // every row between the two clicks.
      onMouseDown={(e) => {
        if (e.shiftKey && onRowItself(e)) e.preventDefault();
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "Home" || e.key === "End") {
          e.preventDefault();
          onNavigate(i, e.key, e.shiftKey);
          return;
        }
        if (e.key !== " " && e.key !== "Enter") return;
        e.preventDefault();
        onToggleSelect(i, { range: e.shiftKey, toggle: e.ctrlKey || e.metaKey });
      }}
      className={cn(
        // The open editor hosts a non-portaled Combobox dropdown that must
        // paint past the row's box - content-visibility's paint containment
        // would clip it, so drop cv-row while this row is being edited.
        !editing && "cv-row",
        "cursor-pointer rounded-md border text-sm transition-colors",
        // Selected first, with Update Test Cases' accent border and tint:
        // the rows being acted on have to be findable at a glance. Then a
        // row that failed to upload, which needs a decision now and
        // outranks where its text last came from.
        isSelected
          ? "border-accent bg-accent-soft"
          : held
          ? "border-warning/60 bg-warning/5"
          : failed
            ? "border-danger/60 bg-danger/5"
            : touched === "added"
              ? "border-success/50 bg-success/5"
              : touched === "changed"
                ? "border-warning/50 bg-warning/5"
                : uploaded
                  ? "border-success/40"
                  : "border-border hover:border-border-strong",
      )}
    >
      <div role="gridcell">
      <div className="flex items-center justify-between gap-3 px-3 py-1.5">
        <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-text">
          <button
            aria-label={stepsOpen ? `Collapse steps of ${tc.title}` : `Expand steps of ${tc.title}`}
            title={stepsOpen ? "Hide steps" : "Check the steps before submitting"}
            className="inline-flex text-muted hover:text-accent"
            onClick={() => onToggleSteps(i)}
          >
            {stepsOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
          {tc.update_id != null ? (
            <Badge id={id("op")} className="bg-warning/20 text-warning">UPDATE #{tc.update_id}</Badge>
          ) : (
            <Badge id={id("op")} className="bg-success/20 text-success">NEW</Badge>
          )}
          {uploaded && (
            <Badge id={id("uploaded")} className="bg-success/20 text-success">
              UPLOADED
            </Badge>
          )}
          <span id={id("title")}>{tc.title}</span>
          {/* A failed row says so in colour only; this says it in words. */}
          {failed && (
            <span id={id("failed")} className="sr-only">
              Upload failed
            </span>
          )}
          <span className="text-xs text-faint">{tc.steps.length} steps</span>
          {held && (
            <span id={id("held")} className="text-xs text-warning">
              {ambiguous
                ? "More than one test case with this title exists in Azure DevOps - check there before uploading again."
                : "Outcome unknown - check before uploading again"}
            </span>
          )}
          {diff?.noop && (
            <Badge id={id("noop")} className="bg-warning/20 text-warning">no-op — nothing will change</Badge>
          )}
          {diff && !diff.noop && (
            <button className="text-xs text-accent hover:underline" onClick={() => onToggleDiff(i)}>
              {diffSummary(diff)} {diffOpen ? "▾" : "▸"}
            </button>
          )}
          {fileChange && tc.update_id == null && (
            <button
              aria-label={`${fileChangeSummary(fileChange)} by the file, in ${tc.title}`}
              aria-expanded={diffOpen}
              className="text-xs text-accent hover:underline"
              onClick={() => onToggleDiff(i)}
            >
              {fileChangeSummary(fileChange)} {diffOpen ? "▾" : "▸"}
            </button>
          )}
          {diffFailed && <span className="text-xs text-faint">diff unavailable</span>}
          {reviewing && problem && (
            <span id={id("review")} className="text-xs text-danger">
              {problem}
            </span>
          )}
          {reviewing && !problem && duplicate && (
            <span id={id("review")} className="text-xs text-warning">
              {duplicate}
            </span>
          )}
        </span>
        <span className="flex shrink-0 items-center gap-3">
          {/* Named for their case: every row has one of each, and a list of
              "Edit, Remove, Edit, Remove" says nothing about which is which. */}
          <button
            aria-label={editing ? `Close the editor for ${tc.title}` : `Edit ${tc.title}`}
            className="text-xs text-faint hover:text-accent disabled:opacity-50"
            disabled={busy}
            onClick={() => onToggleEdit(i)}
          >
            {editing ? "Close" : "Edit"}
          </button>
          <button
            aria-label={`Remove ${tc.title} from the queue`}
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
        <div data-row-ignore className="cursor-auto">
          <QueueCaseEditor
            original={tc}
            org={org}
            project={project}
            onSave={(next) => onSave(i, next)}
            onCancel={onCancelEdit}
          />
        </div>
      )}
      <Collapse open={stepsOpen}>
        <div data-row-ignore className="cursor-auto border-t border-border">
          {/* Shared with the watched-file change report, which
              needed the same "read the case start to finish"
              view - see CaseStepsTable. */}
          <CaseStepsTable steps={tc.steps} preconditions={tc.preconditions} reviewerNotes={tc.reviewer_notes} org={org} />
        </div>
      </Collapse>
      <Collapse open={Boolean(diff && !diff.noop && diffOpen)}>
      {diff && !diff.noop && (
        <div data-row-ignore className="cursor-auto space-y-1 border-t border-border px-3 py-2 text-xs">
          <CaseChangeDetail fields={diff.fields} steps={diff.steps.detail} org={org} />
          {/* A step-type repair has no text change to draw, so it is
              said in words - without this, a case whose only change is
              its step types opened an empty panel. */}
          {diff.steps.retypedDetail.length > 0 && (
            <div className="space-y-0.5">
              <span className="font-medium text-muted">Step types:</span>
              {retypedLines(diff).map((line) => (
                <div key={line} className="text-text">
                  {line}
                </div>
              ))}
              <div className="text-faint">The step text and formatting stay exactly as they are.</div>
            </div>
          )}
          {diff.blankSkipped.length > 0 && (
            <div className="text-faint">Left untouched (blank in import): {diff.blankSkipped.join(", ")}</div>
          )}
        </div>
      )}
      </Collapse>
      <Collapse open={Boolean(showFileDiff && diffOpen)}>
        {showFileDiff && (
          <div data-row-ignore className="cursor-auto space-y-1 border-t border-border px-3 py-2 text-xs">
            <CaseChangeDetail fields={fileChange.fields} steps={fileChange.steps} org={org} />
          </div>
        )}
      </Collapse>
      </div>
    </div>
  );
}

const QueueRow = memo(QueueRowInner);
export default QueueRow;
