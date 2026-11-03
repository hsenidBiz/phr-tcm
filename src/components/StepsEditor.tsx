import { useRef, useState } from "react";
import { GripVertical } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type Step, type StepAttachment } from "../bindings";
import { Button } from "./ui/button";
import { IconAdd, IconAttach, IconRemove } from "../lib/actionIcons";
import { formatBytes } from "../lib/bytes";
import { toast } from "../lib/toast";
import { cn } from "../lib/cn";
import { CaseTextField, PreviewToggle } from "./CaseText";
import SharedStepLabel from "./SharedStepLabel";

/** A step field's height for its text: two lines at least, so a field
 * reads as one that takes more than one; up to seven before it scrolls
 * (or is dragged taller). Whole class names, so Tailwind finds them. */
const FIELD_HEIGHTS = ["h-12", "h-12", "h-12", "h-16", "h-20", "h-24", "h-28", "h-32"];
const fieldHeight = (text: string) =>
  FIELD_HEIGHTS[Math.min(FIELD_HEIGHTS.length - 1, text.split("\n").length)];

/** The step grid used by Manual Entry AND the case editor: numbered rows of
 * action/expected with reorder, remove and Add Step - one interaction model
 * everywhere (the v1 steps table).
 *
 * Reordering is a drag handle, not up/down arrows: moving a step five
 * places was five clicks, each one re-finding the row as it jumped. The
 * handle still moves one place per ArrowUp/ArrowDown when focused, so the
 * keyboard path the arrows provided is not lost - dragging is the fast
 * path, not the only one. */
export default function StepsEditor({
  steps,
  onChange,
  org,
}: {
  steps: Step[];
  onChange: (steps: Step[]) => void;
  /** For the titles of Shared Steps rows; without it only the reference shows. */
  org?: string;
}) {
  const setStep = (i: number, key: "action" | "expected", value: string) =>
    onChange(steps.map((s, j) => (j === i ? { ...s, [key]: value } : s)));

  const move = (i: number, delta: -1 | 1) => {
    const j = i + delta;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };

  /** Take the step at `from` out and re-insert it at `to`. */
  const reorder = (from: number, to: number) => {
    if (from === to || from < 0 || to < 0 || from >= steps.length || to >= steps.length) return;
    const next = [...steps];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    onChange(next);
  };

  // The picker is async: attach to the steps as they are when it closes.
  const stepsRef = useRef(steps);
  stepsRef.current = steps;

  /** Pick files for step `i`. Rust checks them (name, size, the rules for
   * a plain local file under the cap); a refused one is told to the person
   * and adds nothing. Only names and sizes are shown - never the path. */
  const attach = async (i: number) => {
    let checks: Awaited<ReturnType<typeof commands.describeStepFiles>>;
    try {
      const picked = await open({ multiple: true });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (paths.length === 0) return;
      checks = await commands.describeStepFiles(paths);
    } catch {
      toast.error("The files could not be checked. Try again.");
      return;
    }
    const added: StepAttachment[] = [];
    for (const c of checks) {
      if (c.refused) toast.error(c.refused);
      else added.push({ path: c.path, name: c.name, size: c.size });
    }
    if (added.length === 0) return;
    onChange(
      stepsRef.current.map((s, j) =>
        j === i ? { ...s, attachments: [...(s.attachments ?? []), ...added] } : s,
      ),
    );
  };
  // Index being dragged, and the row currently under it. Component state,
  // not dataTransfer: jsdom and some WebViews give dataTransfer trouble,
  // and the indices never need to leave this component anyway.
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  // Step text is Markdown; Preview shows every step formatted at once.
  const [preview, setPreview] = useState(false);

  return (
    <div className="space-y-1">
      {steps.length === 0 && (
        <p className="text-xs text-faint">No steps yet - add the first one below.</p>
      )}
      {steps.map((s, i) => (
        <div
          key={i}
          className={cn(
            "rounded-md border border-transparent",
            // The row under the drag shows where the step will land; the
            // dragged row dims so the eye tracks the gap, not the ghost.
            over === i && dragging !== null && dragging !== i && "border-accent bg-accent-soft",
            dragging === i && "opacity-50",
          )}
          onDragOver={(e) => {
            if (dragging === null) return;
            e.preventDefault(); // required, or drop never fires
            setOver(i);
          }}
          onDrop={(e) => {
            e.preventDefault();
            if (dragging !== null) reorder(dragging, i);
            setDragging(null);
            setOver(null);
          }}
        >
          <div className="flex items-start gap-1">
          <span
            role="button"
            tabIndex={0}
            draggable
            aria-label={`Reorder step ${i + 1}`}
            title="Drag to reorder - or focus and use the arrow keys"
            className="cursor-grab px-0.5 pt-2 text-faint hover:text-text active:cursor-grabbing"
            onDragStart={() => setDragging(i)}
            onDragEnd={() => {
              setDragging(null);
              setOver(null);
            }}
            // The arrows' keyboard path, kept: a handle you can only use
            // with a mouse would trade five clicks for zero keys.
            onKeyDown={(e) => {
              if (e.key === "ArrowUp") {
                e.preventDefault();
                move(i, -1);
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                move(i, 1);
              }
            }}
          >
            <GripVertical size={14} />
          </span>
          <span className="id-mono w-5 pt-2 text-right text-xs text-faint">{i + 1}</span>
          {s.shared != null ? (
            // Locked: a Shared Steps reference is edited in its own work
            // item. It can still be dragged, moved by keyboard and removed.
            <div
              aria-label={`Step ${i + 1} shared steps`}
              title="Shared steps are edited in Azure DevOps - here they can be moved or removed."
              className="flex-1 truncate rounded-md border border-border bg-surface-2 px-2 py-1.5"
            >
              <SharedStepLabel id={s.shared} org={org} />
            </div>
          ) : (
            <>
              <CaseTextField
                kind="step"
                preview={preview}
                aria-label={`Step ${i + 1} action`}
                className={cn("min-w-0 flex-1 text-xs", fieldHeight(s.action))}
                placeholder="Action"
                value={s.action}
                onChange={(v) => setStep(i, "action", v)}
              />
              <CaseTextField
                kind="step"
                preview={preview}
                aria-label={`Step ${i + 1} expected`}
                className={cn("min-w-0 flex-1 text-xs", fieldHeight(s.expected))}
                placeholder="Expected result"
                value={s.expected}
                onChange={(v) => setStep(i, "expected", v)}
              />
            </>
          )}
          <button
            aria-label={`Remove step ${i + 1}`}
            className="px-1 pt-2 text-xs text-faint hover:text-danger"
            title="Remove step"
            onClick={() => onChange(steps.filter((_, j) => j !== i))}
          >
            ✕
          </button>
          </div>
          {s.shared == null && (
            <div className="flex flex-wrap items-center gap-1 pb-1 pl-9">
              {(s.attachments ?? []).map((a, k) => (
                <span
                  key={`${a.id ?? a.path ?? a.name}-${k}`}
                  title={a.name}
                  className="inline-flex max-w-xs items-center gap-1 rounded-md border border-border bg-surface-2 px-1.5 py-0.5 text-xs text-muted"
                >
                  <IconAttach className="size-3" aria-hidden />
                  <span className="truncate">{a.name}</span>
                  {a.size != null && <span className="text-faint">{formatBytes(a.size)}</span>}
                  <button
                    aria-label={`Remove ${a.name} from step ${i + 1}`}
                    title={`Remove ${a.name}`}
                    className="text-faint hover:text-danger"
                    onClick={() =>
                      onChange(
                        steps.map((st, j) =>
                          j === i
                            ? { ...st, attachments: (st.attachments ?? []).filter((_, m) => m !== k) }
                            : st,
                        ),
                      )
                    }
                  >
                    <IconRemove className="size-3" aria-hidden />
                  </button>
                </span>
              ))}
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Attach a file to step ${i + 1}`}
                onClick={() => void attach(i)}
              >
                <IconAttach aria-hidden />
                Attach
              </Button>
            </div>
          )}
        </div>
      ))}
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onChange([...steps, { action: "", expected: "" }])}
        >
          <IconAdd aria-hidden />
          Add Step
        </Button>
        {steps.length > 0 && (
          <PreviewToggle what="steps" on={preview} onToggle={() => setPreview((p) => !p)} />
        )}
      </div>
    </div>
  );
}
