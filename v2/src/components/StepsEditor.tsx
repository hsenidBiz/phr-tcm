import { useState } from "react";
import { GripVertical } from "lucide-react";
import type { Step } from "../bindings";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { IconAdd } from "../lib/actionIcons";
import { cn } from "../lib/cn";

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
}: {
  steps: Step[];
  onChange: (steps: Step[]) => void;
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

  // Index being dragged, and the row currently under it. Component state,
  // not dataTransfer: jsdom and some WebViews give dataTransfer trouble,
  // and the indices never need to leave this component anyway.
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);

  return (
    <div className="space-y-1">
      {steps.length === 0 && (
        <p className="text-xs text-faint">No steps yet - add the first one below.</p>
      )}
      {steps.map((s, i) => (
        <div
          key={i}
          className={cn(
            "flex items-center gap-1 rounded-md border border-transparent",
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
          <span
            role="button"
            tabIndex={0}
            draggable
            aria-label={`Reorder step ${i + 1}`}
            title="Drag to reorder - or focus and use the arrow keys"
            className="cursor-grab px-0.5 text-faint hover:text-text active:cursor-grabbing"
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
          <span className="id-mono w-5 text-right text-xs text-faint">{i + 1}</span>
          <Input
            aria-label={`Step ${i + 1} action`}
            className="flex-1 px-2 py-1.5 text-xs"
            placeholder="Action"
            value={s.action}
            onChange={(e) => setStep(i, "action", e.target.value)}
          />
          <Input
            aria-label={`Step ${i + 1} expected`}
            className="flex-1 px-2 py-1.5 text-xs"
            placeholder="Expected result"
            value={s.expected}
            onChange={(e) => setStep(i, "expected", e.target.value)}
          />
          <button
            className="px-1 text-xs text-faint hover:text-danger"
            title="Remove step"
            onClick={() => onChange(steps.filter((_, j) => j !== i))}
          >
            ✕
          </button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onChange([...steps, { action: "", expected: "" }])}
      >
        <IconAdd aria-hidden />
        + Add Step
      </Button>
    </div>
  );
}
