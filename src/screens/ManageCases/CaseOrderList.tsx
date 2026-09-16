import { ChevronDown, ChevronRight, GripVertical } from "lucide-react";
import { useState, type KeyboardEvent, type MouseEvent } from "react";
import { Button } from "../../components/ui/button";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { IconClear, IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { moveBlock, moveBlockBefore, nudgeBlock, sectionsOf, type SuiteCase } from "../../lib/suiteOrder";

/** A section's display name, which is also its key in the collapsed set. */
export const sectionLabel = (name: string) => name || "Ungrouped";

/** The suite's cases in their current order. Drag a row onto another to put
 * it there; a selected row carries the whole selection with it, in its
 * order. The arrow buttons do the same one step at a time.
 *
 * Selection works the way View Test Cases and Update Test Cases do: click a
 * row to select it, Ctrl+click to add or remove one, Shift+click for a
 * range. Ctrl+click a group header to select or clear the whole group; a
 * plain click on it folds the group. The selection is the one the screen's
 * bulk actions act on, and so does a drag. Native drag events, no library:
 * the app's board already works this way. */
export default function CaseOrderList({
  cases,
  selected,
  onChange,
  onSelect,
  ariaLabel,
  disabled = false,
  grouped = false,
  collapsed = new Set<string>(),
  onToggleCollapsed = () => {},
}: {
  cases: SuiteCase[];
  selected: Set<number>;
  onChange: (next: SuiteCase[]) => void;
  onSelect: (next: Set<number>) => void;
  ariaLabel: string;
  disabled?: boolean;
  /** Show a header per title group (runs of one group in the CURRENT
   * order), each draggable and selectable as a block. Purely a display and
   * interaction change here - whether turning it on rearranges the list
   * is the caller's call (SuiteCases owns that decision). */
  grouped?: boolean;
  /** Section names whose rows are folded away. */
  collapsed?: Set<string>;
  onToggleCollapsed?: (name: string) => void;
}) {
  const [dragId, setDragId] = useState<number | null>(null);
  const [overId, setOverId] = useState<number | null>(null);
  // Where a Shift+click range starts: the last row clicked without Shift.
  const [anchor, setAnchor] = useState<number | null>(null);
  const settled = useSettled(cases.length > 0);
  const sections = grouped ? sectionsOf(cases) : [];
  const isFolded = (name: string) => collapsed.has(sectionLabel(name));

  // A selected row drags its whole selection; an unselected one drags alone.
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
  // A header is a section boundary, not a row: a drop on it must land
  // BEFORE that section whichever direction the drag came from, unlike a
  // row drop (which keeps moveBlock's up/down asymmetry). Without this,
  // a block dragged from above a header landed after the section's first
  // case - inside the section - instead of ahead of it.
  const dropBefore = (targetId: number) => {
    if (dragId == null) return;
    const block = blockFor(dragId);
    if (block.has(targetId)) return;
    onChange(moveBlockBefore(cases, block, targetId));
  };

  const here = cases.filter((c) => selected.has(c.id)).length;
  const indexOf = (id: number) => cases.findIndex((c) => c.id === id);
  // The rows a Shift+click range walks: the ones on screen, in screen
  // order. A folded group's rows are not in it - a range must not quietly
  // pick up cases the user cannot see.
  const onScreen = grouped
    ? sections.flatMap((s) => (isFolded(s.name) ? [] : s.ids))
    : cases.map((c) => c.id);

  const toggleOne = (id: number) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onSelect(next);
  };

  const clickRow = (id: number, e: MouseEvent) => {
    const additive = e.ctrlKey || e.metaKey;
    if (e.shiftKey && anchor != null) {
      const a = onScreen.indexOf(anchor);
      const b = onScreen.indexOf(id);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        const range = onScreen.slice(lo, hi + 1);
        onSelect(additive ? new Set([...selected, ...range]) : new Set(range));
        return;
      }
    }
    if (additive) toggleOne(id);
    // A plain click selects just this row; clicking the only selected row
    // again clears it, same as View Test Cases.
    else onSelect(here === 1 && selected.has(id) ? new Set() : new Set([id]));
    setAnchor(id);
  };

  // Keyboard users get the same selection the checkbox used to give them.
  const keyRow = (id: number, e: KeyboardEvent) => {
    if (e.key !== " " && e.key !== "Enter") return;
    e.preventDefault();
    toggleOne(id);
    setAnchor(id);
  };

  const toggleSection = (ids: number[]) => {
    const all = ids.every((id) => selected.has(id));
    const next = new Set(selected);
    for (const id of ids) {
      if (all) next.delete(id);
      else next.add(id);
    }
    onSelect(next);
    if (ids.length) setAnchor(ids[0]);
  };

  const row = (c: SuiteCase, i: number) => {
    const isOn = selected.has(c.id);
    return (
      <li
        key={c.id}
        data-selected={isOn ? "true" : undefined}
        tabIndex={0}
        draggable={!disabled}
        onClick={(e) => clickRow(c.id, e)}
        onKeyDown={(e) => keyRow(c.id, e)}
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
          "flex cursor-pointer select-none items-center gap-3 border-l-2 px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent/50",
          isOn ? "border-l-accent bg-accent-soft" : "border-l-transparent hover:bg-surface-2",
          !disabled && "active:cursor-grabbing",
          dragId != null && blockFor(dragId).has(c.id) && "opacity-50",
          overId === c.id && dragId !== c.id && "border-t-2 border-t-accent",
        )}
      >
        <GripVertical size={14} className="shrink-0 cursor-grab text-faint" aria-hidden />
        <span className="id-mono w-8 shrink-0 text-right text-faint">{i + 1}</span>
        <span className="id-mono shrink-0 text-faint">#{c.id}</span>
        <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
        <span className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            aria-label={`Move #${c.id} up`}
            title="Move up"
            disabled={disabled || i === 0}
            className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
            onClick={(e) => {
              e.stopPropagation();
              onChange(nudgeBlock(cases, blockFor(c.id), "up"));
            }}
          >
            <IconMoveUp aria-hidden />
          </button>
          <button
            type="button"
            aria-label={`Move #${c.id} down`}
            title="Move down"
            disabled={disabled || i === cases.length - 1}
            className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
            onClick={(e) => {
              e.stopPropagation();
              onChange(nudgeBlock(cases, blockFor(c.id), "down"));
            }}
          >
            <IconMoveDown aria-hidden />
          </button>
        </span>
      </li>
    );
  };

  return (
    <div className="rounded-md border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-muted">
        <span className={cn(here > 0 && "font-medium text-accent")}>
          {here > 0 ? `${here} of ${cases.length} selected` : `${cases.length} test cases`}
        </span>
        <Button
          size="sm"
          variant="ghost"
          disabled={here === cases.length}
          onClick={() => onSelect(new Set([...selected, ...cases.map((c) => c.id)]))}
        >
          Select all
        </Button>
        {here > 0 && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              const next = new Set(selected);
              for (const c of cases) next.delete(c.id);
              onSelect(next);
            }}
          >
            <IconClear aria-hidden />
            Clear
          </Button>
        )}
        <span className="ml-auto text-faint">Ctrl+click to add · Shift+click for a range</span>
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
            const picked = s.ids.filter((id) => selected.has(id)).length;
            const name = sectionLabel(s.name);
            const folded = isFolded(s.name);
            return [
              // role=presentation: the header is a control strip for its
              // section, not one of the suite's cases, so anything that
              // counts listitems (tests, assistive tech) still sees the
              // cases alone.
              <li
                key={`section-${si}-${firstId}`}
                role="presentation"
                draggable={!disabled}
                // A plain click folds the group, like every other grouped
                // list in the app; Ctrl+click selects or clears all of it.
                onClick={(e) => (e.ctrlKey || e.metaKey || e.shiftKey ? toggleSection(s.ids) : onToggleCollapsed(name))}
                onDragStart={() => setDragId(-firstId)}
                onDragEnd={() => {
                  setDragId(null);
                  setOverId(null);
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (overId !== -firstId) setOverId(-firstId);
                }}
                onDragLeave={() => setOverId((o) => (o === -firstId ? null : o))}
                onDrop={(e) => {
                  e.preventDefault();
                  dropBefore(firstId);
                  setDragId(null);
                  setOverId(null);
                }}
                className={cn(
                  "flex cursor-pointer select-none items-center gap-3 border-l-2 px-3 py-1.5 text-xs font-medium text-muted",
                  picked === s.ids.length ? "border-l-accent bg-accent-soft" : "border-l-transparent bg-surface-2/60",
                  overId === -firstId && "border-t-2 border-t-accent",
                )}
              >
                <GripVertical size={14} className="shrink-0 cursor-grab text-faint" aria-hidden />
                <button
                  type="button"
                  aria-label={`${folded ? "Expand" : "Collapse"} group ${name}`}
                  title={folded ? "Expand group" : "Collapse group"}
                  className="text-muted transition-colors hover:text-accent"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleCollapsed(name);
                  }}
                >
                  {folded ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                </button>
                <span className="min-w-0 flex-1 truncate">{name}</span>
                {picked > 0 && (
                  <span className="text-accent">
                    {picked === s.ids.length ? "all selected" : `${picked} selected`}
                  </span>
                )}
                <span className="text-faint">{s.ids.length}</span>
                <span className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    aria-label={`Move group ${name} up`}
                    title="Move group up"
                    disabled={disabled || !prev}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (prev) onChange(moveBlock(cases, members, prev.ids[0]));
                    }}
                  >
                    <IconMoveUp aria-hidden />
                  </button>
                  <button
                    type="button"
                    aria-label={`Move group ${name} down`}
                    title="Move group down"
                    disabled={disabled || !next}
                    className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (next) onChange(moveBlock(cases, members, next.ids[next.ids.length - 1]));
                    }}
                  >
                    <IconMoveDown aria-hidden />
                  </button>
                </span>
              </li>,
              // The section's rows fold as one, in a nested list so the
              // fold has one box to grow and shrink.
              <li key={`rows-${si}-${firstId}`} role="presentation">
                <Collapse open={!folded} animateIn={settled}>
                  <ol className="divide-y divide-border">
                    {s.ids.map((id) => row(cases.find((c) => c.id === id)!, indexOf(id)))}
                  </ol>
                </Collapse>
              </li>,
            ];
          })}
      </ol>
    </div>
  );
}
