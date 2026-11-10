/**
 * The rows of one Run Tests group (backlog A1.5).
 *
 * Each row is memoized: a selection click used to render every row of the
 * suite again, because the rows were inline JSX in RunPanel. Now only the
 * rows whose `selected` or `expanded` flag changed render. Every prop is a
 * value or a stable callback - RunPanel passes its handlers through refs.
 *
 * A long group renders only the rows near the visible part of the page
 * (`useRowWindow`). Each row and its preview sit in a <tbody> of their own,
 * which is what the window measures; a table may hold any number of them.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { Fragment, memo, type MouseEvent } from "react";
import type { RunOutcome, TestPoint } from "../../bindings";
import HistoryDots from "../../components/HistoryDots";
import { Collapse } from "../../components/ui/collapse";
import { useRowWindow } from "../../components/ui/useRowWindow";
import { cn } from "../../lib/cn";
import { outcomeLabel } from "../../lib/outcomes";
import CasePreview from "./CasePreview";

/** One collapsed row is about this tall; expanded rows are measured. */
const ROW_ESTIMATE_PX = 29;
const NO_HISTORY: RunOutcome[] = [];

type RowProps = {
  point: TestPoint;
  index: number;
  selected: boolean;
  expanded: boolean;
  history: RunOutcome[];
  org: string;
  project: string;
  rowTint: Record<string, string>;
  outcomeColor: Record<string, string>;
  onRowClick: (p: TestPoint, e: MouseEvent) => void;
  onToggleExpanded: (pointId: number) => void;
  measure: (el: Element | null) => void;
};

const RunRow = memo(function RunRow({
  point: p,
  index,
  selected,
  expanded,
  history,
  org,
  project,
  rowTint,
  outcomeColor,
  onRowClick,
  onToggleExpanded,
  measure,
}: RowProps) {
  return (
    <tbody ref={measure} data-index={index}>
      <tr
        className={cn(
          "cursor-pointer select-none border-b border-border/50 transition-colors",
          selected ? "bg-accent-soft" : (rowTint[p.last_outcome.toLowerCase()] ?? ""),
          // The Paused and Not Applicable tints are already close to
          // surface-2, so the hover tint alone barely showed on them
          // (owner, 2026-10-27). An accent bar at the row's start
          // marks the hovered row on every tint, selected included.
          "hover:bg-surface-2 hover:shadow-[inset_3px_0_0_var(--color-accent)]",
        )}
        onClick={(e) => onRowClick(p, e)}
      >
        <td className="px-2 py-1 text-text">
          <div className="flex items-center gap-1">
            <button
              aria-label={`${expanded ? "Collapse" : "Expand"} test case #${p.test_case_id}`}
              title={expanded ? "Hide steps" : "Show steps & last result"}
              className="shrink-0 text-muted hover:text-accent"
              onClick={(e) => {
                e.stopPropagation();
                onToggleExpanded(p.point_id);
              }}
            >
              {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </button>
            <span className="min-w-0 flex-1">
              <span className="id-mono text-faint">#{p.test_case_id}</span> {p.test_case_name}
            </span>
          </div>
        </td>
        <td className={cn("px-2 py-1", outcomeColor[p.last_outcome.toLowerCase()] ?? "text-faint")}>
          {outcomeLabel(p.last_outcome) || "—"}
        </td>
        <td className="px-2 py-1">
          <HistoryDots outcomes={history} />
        </td>
      </tr>
      <Collapse row={3} open={expanded}>
        <CasePreview org={org} project={project} point={p} history={history} />
      </Collapse>
    </tbody>
  );
});

export function RunRows({
  rows,
  selected,
  expanded,
  historyByCase,
  org,
  project,
  rowTint,
  outcomeColor,
  onRowClick,
  onToggleExpanded,
}: {
  rows: TestPoint[];
  selected: Set<number>;
  expanded: Set<number>;
  historyByCase: Map<number, RunOutcome[]>;
  org: string;
  project: string;
  rowTint: Record<string, string>;
  outcomeColor: Record<string, string>;
  onRowClick: (p: TestPoint, e: MouseEvent) => void;
  onToggleExpanded: (pointId: number) => void;
}) {
  // Expanded rows stay rendered: closing them is the reader's choice.
  const pinned = rows.flatMap((p, i) => (expanded.has(p.point_id) ? [i] : []));
  const win = useRowWindow(rows.length, {
    estimate: ROW_ESTIMATE_PX,
    keyOf: (i) => rows[i].point_id,
    pinned,
  });
  return (
    <>
      {/* The list's own anchor, for the window to find where it starts. */}
      <tbody ref={win.listRef} aria-hidden />
      {win.items.map(({ index, space }) => {
        const p = rows[index];
        return (
          <Fragment key={p.point_id}>
            {space > 0 && (
              <tbody aria-hidden>
                <tr style={{ height: space }} />
              </tbody>
            )}
            <RunRow
              point={p}
              index={index}
              selected={p.test_case_id != null && selected.has(p.test_case_id)}
              expanded={expanded.has(p.point_id)}
              history={p.test_case_id != null ? (historyByCase.get(p.test_case_id) ?? NO_HISTORY) : NO_HISTORY}
              org={org}
              project={project}
              rowTint={rowTint}
              outcomeColor={outcomeColor}
              onRowClick={onRowClick}
              onToggleExpanded={onToggleExpanded}
              measure={win.measure}
            />
          </Fragment>
        );
      })}
      {win.after > 0 && (
        <tbody aria-hidden>
          <tr style={{ height: win.after }} />
        </tbody>
      )}
    </>
  );
}
