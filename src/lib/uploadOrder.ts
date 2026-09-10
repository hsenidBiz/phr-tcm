// The order cases are uploaded in IS the order they land in the suite:
// the submit creates them one by one, and Azure DevOps lists a
// requirement suite's cases as they were added. So the send list, not the
// screen, is where "tester order" has to be applied.

import type { TestCase } from "../bindings";

/** The send list in tester order - the run-sheet order an optimized draft
 * carries in `tester_order` - when every case has one. A mixed list (some
 * cases never optimized, typed by hand, or from an older file) is left in
 * the order on screen: sorting half a queue would interleave the ordered
 * cases with unordered ones at arbitrary points, and the "Order:" bar
 * above the queue already says why the tester button is off. */
export function orderForUpload(cases: TestCase[]): TestCase[] {
  if (cases.length < 2 || !cases.every((tc) => tc.tester_order != null)) return cases;
  // Stable: two cases sharing a rank keep their screen order.
  return cases
    .map((tc, i) => ({ tc, i }))
    .sort((a, b) => (a.tc.tester_order! - b.tc.tester_order!) || a.i - b.i)
    .map(({ tc }) => tc);
}
