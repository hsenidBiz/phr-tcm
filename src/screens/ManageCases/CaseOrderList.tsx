import { GripVertical } from "lucide-react";
import { useState } from "react";
import { Checkbox } from "../../components/ui/checkbox";
import { IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { moveBlock, nudgeBlock, type SuiteCase } from "../../lib/suiteOrder";

/** The suite's cases in their current order. Drag a row onto another to put
 * it there; a ticked row carries the whole selection with it, in its order.
 * The arrow buttons do the same one step at a time (and are what a keyboard
 * user gets). The checkbox is the one selection the screen has: the bulk
 * actions act on it, and so does a drag. Native drag events, no library:
 * the app's board already works this way. */
export default function CaseOrderList({
  cases,
  selected,
  onChange,
  onSelect,
  ariaLabel,
  disabled = false,
}: {
  cases: SuiteCase[];
  selected: Set<number>;
  onChange: (next: SuiteCase[]) => void;
  onSelect: (next: Set<number>) => void;
  ariaLabel: string;
  disabled?: boolean;
}) {
  const [dragId, setDragId] = useState<number | null>(null);
  const [overId, setOverId] = useState<number | null>(null);

  // A ticked row drags its whole selection; an unticked one drags alone.
  const blockFor = (id: number): ReadonlySet<number> => (selected.has(id) ? selected : new Set([id]));
  const dropOn = (targetId: number) => {
    if (dragId == null || dragId === targetId) return;
    onChange(moveBlock(cases, blockFor(dragId), targetId));
  };
  const toggle = (id: number, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(id);
    else next.delete(id);
    onSelect(next);
  };
  const allOn = cases.length > 0 && cases.every((c) => selected.has(c.id));
  const someOn = cases.some((c) => selected.has(c.id));
  const here = cases.filter((c) => selected.has(c.id)).length;

  return (
    <div className="rounded-md border border-border bg-surface">
      <div className="flex items-center gap-3 border-b border-border px-3 py-2 text-xs text-muted">
        <Checkbox
          checked={allOn}
          indeterminate={!allOn && someOn}
          onCheckedChange={(on) => onSelect(on ? new Set(cases.map((c) => c.id)) : new Set())}
          ariaLabel="Select all test cases"
        />
        <span>
          {here > 0 ? `${here} of ${cases.length} selected` : `${cases.length} test cases`}
        </span>
      </div>
      <ol aria-label={ariaLabel} className="divide-y divide-border">
        {cases.map((c, i) => (
          <li
            key={c.id}
            draggable={!disabled}
            onDragStart={() => setDragId(c.id)}
            onDragEnd={() => {
              setDragId(null);
              setOverId(null);
            }}
            onDragOver={(e) => {
              e.preventDefault();
              if (overId !== c.id) setOverId(c.id);
            }}
            onDragLeave={() => setOverId((o) => (o === c.id ? null : o))}
            onDrop={(e) => {
              e.preventDefault();
              dropOn(c.id);
              setDragId(null);
              setOverId(null);
            }}
            className={cn(
              "flex items-center gap-3 px-3 py-2 text-sm",
              !disabled && "cursor-grab hover:bg-surface-2",
              dragId != null && blockFor(dragId).has(c.id) && "opacity-50",
              overId === c.id && dragId !== c.id && "border-t-2 border-accent",
            )}
          >
            <GripVertical size={14} className="shrink-0 text-faint" aria-hidden />
            <span className="id-mono w-8 shrink-0 text-right text-faint">{i + 1}</span>
            <Checkbox
              checked={selected.has(c.id)}
              onCheckedChange={(on) => toggle(c.id, on)}
              ariaLabel={`Select #${c.id}`}
            />
            <span className="id-mono shrink-0 text-faint">#{c.id}</span>
            <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
            <span className="flex shrink-0 items-center gap-1">
              <button
                type="button"
                aria-label={`Move #${c.id} up`}
                title="Move up"
                disabled={disabled || i === 0}
                className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                onClick={() => onChange(nudgeBlock(cases, blockFor(c.id), "up"))}
              >
                <IconMoveUp aria-hidden />
              </button>
              <button
                type="button"
                aria-label={`Move #${c.id} down`}
                title="Move down"
                disabled={disabled || i === cases.length - 1}
                className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                onClick={() => onChange(nudgeBlock(cases, blockFor(c.id), "down"))}
              >
                <IconMoveDown aria-hidden />
              </button>
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}
