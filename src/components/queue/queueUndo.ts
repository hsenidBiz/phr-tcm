import { useCallback, useRef } from "react";
import type { TestCase } from "../../bindings";
import { toast } from "../../lib/toast";

/** One row taken out of the queue: where it stood, and the row itself. The
 * row object is kept, not a copy, so a restored row is the same row - its
 * draft content, step attachments and everything derived from them. */
export type RemovedRow = { index: number; row: TestCase };

/** The rows back where they stood, clamped to the list as it is now, so
 * anything added or changed since stays. Rows go back in ascending order of
 * their old position, which makes an untouched list come out exactly as it
 * was and puts "everything removed" ahead of rows added afterwards. A row
 * already present (the same object) is not put in twice. */
export function restoreRows(queue: TestCase[], removed: RemovedRow[]): TestCase[] {
  const out = [...queue];
  const present = new Set(queue);
  for (const { index, row } of [...removed].sort((a, b) => a.index - b.index)) {
    if (present.has(row)) continue;
    out.splice(Math.min(index, out.length), 0, row);
    present.add(row);
  }
  return out;
}

/** What a bulk edit did to one row: the row before, the row it became, and
 * the fields that differ. */
export type BulkChange = { index: number; before: TestCase; after: TestCase; keys: string[] };

type Loose = Record<string, unknown>;

/** The fields a bulk edit set - compared by value, since a step list or a
 * tag string is rebuilt rather than mutated. */
export function changedKeys(before: TestCase, after: TestCase): string[] {
  const b = before as unknown as Loose;
  const a = after as unknown as Loose;
  return [...new Set([...Object.keys(b), ...Object.keys(a)])].filter(
    (k) => JSON.stringify(b[k]) !== JSON.stringify(a[k]),
  );
}

/** Each bulk-edited row's place in the queue as it is now. By identity
 * first, following the row through the hand edits that replaced it
 * (`successor`); failing that by title, at the old position when it still
 * fits. A row that was renamed some other way is not found and is left as
 * it is. */
function locate(
  queue: TestCase[],
  change: BulkChange,
  successor: WeakMap<TestCase, TestCase>,
  claimed: Set<number>,
): number {
  const chain: TestCase[] = [change.after];
  for (let next = successor.get(change.after); next && chain.length < 1000; next = successor.get(next)) {
    chain.push(next);
  }
  for (let c = chain.length - 1; c >= 0; c--) {
    const at = queue.indexOf(chain[c]);
    if (at >= 0 && !claimed.has(at)) return at;
  }
  const title = change.after.title;
  const home = queue[change.index];
  if (home && home.title === title && !claimed.has(change.index)) return change.index;
  return queue.findIndex((t, i) => t.title === title && !claimed.has(i));
}

/** Undo a bulk edit: on the rows that still exist, put back only the fields
 * the edit set, and only where the field still holds what the edit wrote -
 * a later hand edit wins. Returns the new queue and the positions touched. */
export function revertBulk(
  queue: TestCase[],
  changes: BulkChange[],
  successor: WeakMap<TestCase, TestCase>,
): { queue: TestCase[]; touched: Set<number> } {
  const out = [...queue];
  const touched = new Set<number>();
  const claimed = new Set<number>();
  for (const change of changes) {
    const at = locate(queue, change, successor, claimed);
    if (at < 0) continue;
    claimed.add(at);
    const row = { ...(out[at] as unknown as Loose) };
    const after = change.after as unknown as Loose;
    const before = change.before as unknown as Loose;
    let moved = false;
    for (const k of change.keys) {
      if (JSON.stringify(row[k]) !== JSON.stringify(after[k])) continue;
      if (k in before) row[k] = before[k];
      else delete row[k];
      moved = true;
    }
    if (!moved) continue;
    out[at] = row as unknown as TestCase;
    touched.add(at);
  }
  return { queue: out, touched };
}

/** The queue's one Undo toast. Only the newest destructive action can be
 * undone: showing a new toast takes the older one down and its button, if
 * it is somehow still clicked, does nothing. Undo runs once. */
export function useUndoToast() {
  const live = useRef<{ token: number; id?: string } | null>(null);
  const issued = useRef(0);
  return useCallback(
    (kind: "info" | "success", message: string, undo?: () => void) => {
      if (live.current?.id) toast.dismiss(live.current.id);
      const token = ++issued.current;
      live.current = { token };
      if (!undo) {
        toast[kind](message);
        return;
      }
      const id = toast[kind](message, {
        action: {
          label: "Undo",
          onClick: () => {
            if (live.current?.token !== token) return;
            live.current = null;
            undo();
          },
        },
        duration: 10_000,
      });
      if (live.current?.token === token) live.current.id = id;
    },
    [],
  );
}
