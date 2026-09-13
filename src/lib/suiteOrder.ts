/** One test case as the Manage Test Cases list shows it. */
export type SuiteCase = { id: number; title: string };

/** A copy of `list` with the item at `from` moved to `to`. Anything out of
 * range, or a move onto itself, returns an equal copy. */
export function moveItem<T>(list: T[], from: number, to: number): T[] {
  const out = list.slice();
  if (from === to || from < 0 || to < 0 || from >= out.length || to >= out.length) return out;
  const [item] = out.splice(from, 1);
  out.splice(to, 0, item);
  return out;
}

/** True when both lists carry the same ids in the same order. Titles do
 * not matter: the order is what gets saved. */
export function sameOrder(a: SuiteCase[], b: SuiteCase[]): boolean {
  return a.length === b.length && a.every((c, i) => c.id === b[i].id);
}
