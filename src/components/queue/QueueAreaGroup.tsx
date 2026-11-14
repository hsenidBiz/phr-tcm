import { ChevronDown, ChevronRight } from "lucide-react";
import type { Dispatch, MutableRefObject, ReactNode, SetStateAction } from "react";
import { groupIndices, type AreaGroup } from "../../lib/areaGroups";
import { Checkbox } from "../ui/checkbox";
import { Collapse } from "../ui/collapse";

/**
 * One area group: its heading, its own cases, then its subgroups, each
 * level indented under the last. `hidden` is true inside a folded
 * ancestor.
 *
 * Folding never unmounts an open editor - folding away unsaved edits
 * would be a silent discard (the rule Edit Existing's groups follow). So
 * a folded group still shows the one row being edited, and the path of
 * headings down to it, and nothing else.
 */
export default function QueueAreaGroup({
  group: g,
  hidden,
  foldedAreas,
  toggleFoldedArea,
  editingIdx,
  selected,
  setSelected,
  selAnchor,
  renderRow,
}: {
  group: AreaGroup;
  hidden: boolean;
  foldedAreas: ReadonlySet<string>;
  toggleFoldedArea: (key: string) => void;
  editingIdx: number | null;
  selected: Set<number>;
  setSelected: Dispatch<SetStateAction<Set<number>>>;
  selAnchor: MutableRefObject<number | null>;
  /** One queued case, by its real queue index. */
  renderRow: (i: number) => ReactNode;
}) {
  const folded = foldedAreas.has(g.key);
  const away = hidden || folded;
  const all = groupIndices(g);
  const keep = (i: number) => !away || i === editingIdx;
  const rows = g.indices.filter(keep);
  const kids = away ? g.children.filter((c) => editingIdx != null && groupIndices(c).includes(editingIdx)) : g.children;
  const picked = all.filter((i) => selected.has(i)).length;
  // The group's tick box selects every case in it, nested included, and
  // clears them when they are all selected - Update Test Cases' group
  // tick box (its toggleGroup).
  const pickGroup = () => {
    setSelected((s) => {
      const next = new Set(s);
      const every = all.every((i) => s.has(i));
      for (const i of all) {
        if (every) next.delete(i);
        else next.add(i);
      }
      return next;
    });
    if (all.length) selAnchor.current = all[0];
  };
  return (
    <div className="space-y-1">
      <div className="flex w-full items-center gap-3 pb-1 pt-2">
        <button
          type="button"
          aria-label={`${folded ? "Expand" : "Collapse"} area ${g.path}`}
          aria-expanded={!folded}
          title={folded ? "Expand group" : "Collapse group"}
          className="text-muted transition-colors hover:text-accent"
          onClick={() => toggleFoldedArea(g.key)}
        >
          {folded ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
        </button>
        {/* As on Update Test Cases: the tick box selects the group, and
            the name folds it, same as the chevron. */}
        <Checkbox
          ariaLabel={`Select all in ${g.path}`}
          checked={all.length > 0 && picked === all.length}
          indeterminate={picked > 0 && picked < all.length}
          onCheckedChange={pickGroup}
        />
        <button
          type="button"
          className="group flex items-center gap-2"
          title={folded ? "Expand group" : "Collapse group"}
          onClick={() => toggleFoldedArea(g.key)}
        >
          <span className="text-sm font-semibold text-muted transition-colors group-hover:text-accent">
            {/* The bucket for cases with no area, in the faint colour, so
                a real area that happens to be called "No area" reads apart. */}
            <span
              className={g.key === "" ? "text-faint" : undefined}
              title={g.key === "" ? "Cases with no area" : undefined}
            >
              {g.name}
            </span>{" "}
            ({g.count})
          </span>
        </button>
        {/* A folded group says how much of the selection it hides. */}
        {folded && picked > 0 && <span className="text-xs text-accent">{picked} selected</span>}
        <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
      </div>
      <Collapse open={rows.length + kids.length > 0} animateIn={false}>
        <div className="space-y-1 pl-4">
          {rows.length > 0 && (
            <div role="grid" aria-multiselectable="true" aria-label={`Cases in ${g.path}`} className="space-y-1">
              {rows.map((i) => renderRow(i))}
            </div>
          )}
          {kids.map((c) => (
            <QueueAreaGroup
              key={c.key}
              group={c}
              hidden={away}
              foldedAreas={foldedAreas}
              toggleFoldedArea={toggleFoldedArea}
              editingIdx={editingIdx}
              selected={selected}
              setSelected={setSelected}
              selAnchor={selAnchor}
              renderRow={renderRow}
            />
          ))}
        </div>
      </Collapse>
    </div>
  );
}
