import { describe, expect, test } from "vitest";
import { selectedIdsIn, toggleSelection } from "./selection";

const c = (id: number) => ({ id, title: `T${id}` });

describe("toggleSelection", () => {
  test("adds and removes cases within one plan", () => {
    let sel = toggleSelection(null, 9, [c(1), c(2)], true);
    expect(sel?.planId).toBe(9);
    expect([...sel!.cases.keys()]).toEqual([1, 2]);
    sel = toggleSelection(sel, 9, [c(1)], false);
    expect([...sel!.cases.keys()]).toEqual([2]);
  });
  test("selecting in another plan replaces the selection", () => {
    const sel = toggleSelection(toggleSelection(null, 9, [c(1)], true), 10, [c(5)], true);
    expect(sel?.planId).toBe(10);
    expect([...sel!.cases.keys()]).toEqual([5]);
  });
  test("removing the last case clears the selection", () => {
    const sel = toggleSelection(toggleSelection(null, 9, [c(1)], true), 9, [c(1)], false);
    expect(sel).toBeNull();
  });
  test("removing from another plan is a no-op", () => {
    const before = toggleSelection(null, 9, [c(1)], true);
    expect(toggleSelection(before, 10, [c(1)], false)).toBe(before);
  });
});

describe("selectedIdsIn", () => {
  test("is the plan's ids, or empty for another plan or no selection", () => {
    const sel = toggleSelection(null, 9, [c(1), c(2)], true);
    expect([...selectedIdsIn(sel, 9)]).toEqual([1, 2]);
    expect(selectedIdsIn(sel, 10).size).toBe(0);
    expect(selectedIdsIn(null, 9).size).toBe(0);
  });
});
