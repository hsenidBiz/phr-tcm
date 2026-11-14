import { useCallback, useEffect, useState } from "react";
import type { TestCase } from "../../bindings";
import type { SyncChange } from "../../lib/fileSync";

/** Which rows are open: the inline editor, the steps preview and the diff. */
export function useRowViewState(
  queue: TestCase[],
  fileChanges: Record<string, SyncChange> | undefined,
  rowKeys: string[],
) {
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());
  const toggleDiff = useCallback(
    (i: number) =>
      setExpandedDiffs((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      }),
    [],
  );
  // One row at a time is editable in place; queue-length changes (remove,
  // import, submit) shift indices, so any of them closes the editor.
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  useEffect(() => {
    setEditingIdx(null);
  }, [queue.length]);

  // Per-row steps preview (collapsed by default): check what will actually
  // be written before submitting, for creates and updates alike.
  const [expandedSteps, setExpandedSteps] = useState<Set<number>>(new Set());
  const toggleSteps = useCallback(
    (i: number) =>
      setExpandedSteps((s) => {
        const next = new Set(s);
        if (next.has(i)) next.delete(i);
        else next.add(i);
        return next;
      }),
    [],
  );

  // A NEW row's open diff belongs to the change report that put it there:
  // once the report is dismissed the row has no diff to show, so its open
  // flag goes too (it would otherwise still count towards Collapse all).
  useEffect(() => {
    setExpandedDiffs((s) => {
      let next: Set<number> | null = null;
      for (const i of s) {
        const row = queue[i];
        if (row && row.update_id == null && !fileChanges?.[rowKeys[i]]) {
          next ??= new Set(s);
          next.delete(i);
        }
      }
      return next ?? s;
    });
  }, [fileChanges, queue, rowKeys]);

  const toggleEdit = useCallback((i: number) => setEditingIdx((cur) => (cur === i ? null : i)), []);
  const cancelEdit = useCallback(() => setEditingIdx(null), []);

  return {
    expandedDiffs,
    setExpandedDiffs,
    toggleDiff,
    expandedSteps,
    setExpandedSteps,
    toggleSteps,
    editingIdx,
    setEditingIdx,
    toggleEdit,
    cancelEdit,
  };
}
