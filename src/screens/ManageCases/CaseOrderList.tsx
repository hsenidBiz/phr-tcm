import { GripVertical } from "lucide-react";
import { useState } from "react";
import { Checkbox } from "../../components/ui/checkbox";
import { IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { moveBlock, nudgeBlock, sectionsOf, type SuiteCase } from "../../lib/suiteOrder";

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
  grouped = false,
}: {
  cases: SuiteCase[];
  selected: Set<number>;
  onChange: (next: SuiteCase[]) => void;
  onSelect: (next: Set<number>) => void;
  ariaLabel: string;
  disabled?: boolean;
  /** Show a header per title group (runs of one group in the CURRENT
   * order), each draggable and tickable as a block. Purely a display and
   * interaction change here - whether turning it on rearranges the list
   * is the caller's call (SuiteCases owns that decision). */
  grouped?: boolean;
}) {
  const [dragId, setDragId] = useState<number | null>(null);
  const [overId, setOverId] = useState<number | null>(null);
  const sections = grouped ? sectionsOf(cases) : [];

  // A ticked row drags its whole selection; an unticked one drags alone.
  // A negative dragId marks a section header: -dragId is that section's
  // first case id, so its members are looked up from `sections` rather
  // than calling sectionsOf again.
  const blockFor = (dragged: number): ReadonlySet<number> => {
    if (dragged < 0) {
      const s = sections.find((x) => x.ids[0] === -dragged);
      return new Set(s?.ids ?? []);
    }
    return selected.has(dragged) ? selected : new Set([dragged]);
  };
  const dropOn = (targetId: number) => {
    if (dragId == null) return;
    const block = blockFor(dragId);
    if (block.has(targetId)) return;
    onChange(moveBlock(cases, block, targetId));
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
  const indexOf = (id: number) => cases.findIndex((c) => c.id === id);

  const row = (c: SuiteCase, i: number) => (
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
  );

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
        {!grouped && cases.map((c, i) => row(c, i))}
        {grouped &&
          sections.map((s, si) => {
            const members = new Set(s.ids);
            const firstId = s.ids[0];
            // The arrows move a section past its NEIGHBOUR section, not one
            // case: up lands before the previous section's first case, down
            // after the next section's last.
            const prev = sections[si - 1];
            const next = sections[si + 1];
            const allTicked = s.ids.every((id) => selected.has(id));
            const someTicked = s.ids.some((id) => selected.has(id));
            const name = s.name || "Ungrouped";
            return [
              // role=presentation: the header is a control strip for its
              // section, not one of the suite's cases, so anything that
              // counts listitems (tests, assistive tech) still sees the
              // cases alone.
              <li
                key={`section-${si}-${firstId}`}
                role="presentation"
                draggable={!disabled}
                onDragStart={() => setDragId(-firstId)}
                onDragEnd={() => {
                  setDragId(null);
                  setOverId(null);
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (overId !== firstId) setOverId(firstId);
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  dropOn(firstId);
                  setDragId(null);
                  setOverId(null);
                }}
                className={cn(
                  "flex items-center gap-3 bg-surface-2/60 px-3 py-1.5 text-xs font-medium text-muted",
                  !disabled && "cursor-grab",
                  overId === firstId && "border-t-2 border-accent",
                )}
              >
                <GripVertical size={14} className="shrink-0 text-faint" aria-hidden />
                <Checkbox
                  checked={allTicked}
                  indeterminate={!allTicked && someTicked}
                  onCheckedChange={(on) => {
                    const next = new Set(selected);
                    for (const id of s.ids) on ? next.add(id) : next.delete(id);
                    onSelect(next);
                  }}
                  ariaLabel={`Select group ${name}`}
                />
                <span className="min-w-0 flex-1 truncate">{name}</span>
                <span className="text-faint">{s.ids.length}</span>
                <span className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    aria-label={`Move group ${name} up`}
                    title="Move group up"
                    disabled={disabled || !prev}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={() => prev && onChange(moveBlock(cases, members, prev.ids[0]))}
                  >
                    <IconMoveUp aria-hidden />
                  </button>
                  <button
                    type="button"
                    aria-label={`Move group ${name} down`}
                    title="Move group down"
                    disabled={disabled || !next}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={() => next && onChange(moveBlock(cases, members, next.ids[next.ids.length - 1]))}
                  >
                    <IconMoveDown aria-hidden />
                  </button>
                </span>
              </li>,
              ...s.ids.map((id) => row(cases.find((c) => c.id === id)!, indexOf(id))),
            ];
          })}
      </ol>
    </div>
  );
}
