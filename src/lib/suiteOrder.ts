/** One test case as the Suite Management list shows it. */
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

/** The order a draft file asks for. Cases the file names by id AND gives
 * a `tester_order` come first, sorted by it (ties keep the file's own
 * order); every other case in the suite follows in the order it had.
 * `matched` is how many suite cases the file placed, so the screen can
 * say when a file had nothing to say about this suite. */
export function orderFromFile(
  current: SuiteCase[],
  fileCases: Array<{ update_id: number | null; tester_order?: number | null }>,
): { order: SuiteCase[]; matched: number } {
  const byId = new Map(current.map((c) => [c.id, c]));
  const placed = fileCases
    .map((f, i) => ({ f, i }))
    .filter(({ f }) => f.update_id != null && f.tester_order != null && byId.has(f.update_id))
    .sort((a, b) => a.f.tester_order! - b.f.tester_order! || a.i - b.i)
    .map(({ f }) => byId.get(f.update_id!)!);
  const seen = new Set(placed.map((c) => c.id));
  const first = placed.filter((c, i) => placed.findIndex((p) => p.id === c.id) === i);
  const rest = current.filter((c) => !seen.has(c.id));
  return { order: [...first, ...rest], matched: first.length };
}
