import { act, renderHook } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, test } from "vitest";
import {
  clickHow,
  rangeBetween,
  replaceWithKey,
  selectionAfterClick,
  selectionAfterRange,
  toggleKey,
  toggleMany,
  useRowSelection,
  type RowSelectionOptions,
} from "./useRowSelection";

const click = { range: false, toggle: false };
const ctrl = { range: false, toggle: true };
const shift = { range: true, toggle: false };
const ctrlShift = { range: true, toggle: true };

describe("pure helpers", () => {
  test("clickHow reads shift, and ctrl or meta", () => {
    expect(clickHow({ shiftKey: true, ctrlKey: false, metaKey: false })).toEqual(shift);
    expect(clickHow({ shiftKey: false, ctrlKey: true, metaKey: false })).toEqual(ctrl);
    expect(clickHow({ shiftKey: false, ctrlKey: false, metaKey: true })).toEqual(ctrl);
    expect(clickHow({ shiftKey: false, ctrlKey: false, metaKey: false })).toEqual(click);
  });

  test("rangeBetween runs forward and backward, inclusive", () => {
    const order = [10, 20, 30, 40, 50];
    expect(rangeBetween(order, 20, 40)).toEqual([20, 30, 40]);
    expect(rangeBetween(order, 40, 20)).toEqual([20, 30, 40]);
    expect(rangeBetween(order, 30, 30)).toEqual([30]);
  });

  test("rangeBetween is null when either end is not in the order", () => {
    expect(rangeBetween([1, 2, 3], 9, 2)).toBeNull();
    expect(rangeBetween([1, 2, 3], 2, 9)).toBeNull();
  });

  test("toggleKey flips one key and leaves its input alone", () => {
    const sel = new Set([1, 2]);
    expect([...toggleKey(sel, 3)]).toEqual([1, 2, 3]);
    expect([...toggleKey(sel, 1)]).toEqual([2]);
    expect([...sel]).toEqual([1, 2]);
  });

  test("replaceWithKey selects only the key, and clears when it was alone", () => {
    expect([...replaceWithKey(new Set([1, 2]), 2)]).toEqual([2]);
    expect([...replaceWithKey(new Set([2]), 2)]).toEqual([]);
    expect([...replaceWithKey(new Set<number>(), 2)]).toEqual([2]);
  });

  test("toggleMany adds all when any is missing and clears all when none is", () => {
    expect([...toggleMany(new Set([1]), [1, 2, 3])].sort()).toEqual([1, 2, 3]);
    expect([...toggleMany(new Set([1, 2, 3, 9]), [1, 2, 3])]).toEqual([9]);
  });

  test("selectionAfterClick: replace mode vs toggle mode", () => {
    const sel = new Set([1, 2]);
    expect([...selectionAfterClick(sel, 3, click)]).toEqual([3]);
    expect([...selectionAfterClick(sel, 3, ctrl)]).toEqual([1, 2, 3]);
    expect([...selectionAfterClick(sel, 3, click, "toggle")]).toEqual([1, 2, 3]);
    expect([...selectionAfterClick(sel, 2, click, "toggle")]).toEqual([1]);
  });

  test("selectionAfterRange: replaces, adds with toggle, or always adds", () => {
    const sel = new Set([9]);
    expect([...selectionAfterRange(sel, [1, 2], shift)]).toEqual([1, 2]);
    expect([...selectionAfterRange(sel, [1, 2], ctrlShift)]).toEqual([9, 1, 2]);
    expect([...selectionAfterRange(sel, [1, 2], shift, "add")]).toEqual([9, 1, 2]);
  });
});

/** The hook the way a screen holds it: the set (and optionally the anchor) in
 * state, the order passed in per render. */
function setup(initialOrder: number[], opts: RowSelectionOptions = {}, keepAnchor = false) {
  return renderHook(
    ({ order }: { order: number[] }) => {
      const [selected, setSelected] = useState<Set<number>>(new Set());
      const anchorPair = useState<number | null>(null);
      const api = useRowSelection<number>({
        order,
        setSelected,
        anchor: keepAnchor ? anchorPair : undefined,
        ...opts,
      });
      return { selected, anchor: anchorPair[0], ...api };
    },
    { initialProps: { order: initialOrder } },
  );
}
const ids = (r: { current: { selected: Set<number> } }) => [...r.current.selected].sort((a, b) => a - b);

describe("useRowSelection", () => {
  test("a click selects one row and a second click on it clears it", () => {
    const { result } = setup([1, 2, 3]);
    act(() => result.current.select(2, click));
    expect(ids(result)).toEqual([2]);
    act(() => result.current.select(3, click));
    expect(ids(result)).toEqual([3]);
    act(() => result.current.select(3, click));
    expect(ids(result)).toEqual([]);
  });

  test("Ctrl-click toggles a row and keeps the others", () => {
    const { result } = setup([1, 2, 3]);
    act(() => result.current.select(1, click));
    act(() => result.current.select(3, ctrl));
    expect(ids(result)).toEqual([1, 3]);
    act(() => result.current.select(1, ctrl));
    expect(ids(result)).toEqual([3]);
  });

  test("Shift-click selects forward and backward from the anchor", () => {
    const { result } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(2, click));
    act(() => result.current.select(4, shift));
    expect(ids(result)).toEqual([2, 3, 4]);
    act(() => result.current.select(1, shift));
    expect(ids(result)).toEqual([1, 2]);
  });

  test("a range does not move the anchor", () => {
    const { result } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(2, click));
    act(() => result.current.select(5, shift));
    act(() => result.current.select(3, shift));
    expect(ids(result)).toEqual([2, 3]);
  });

  test("Ctrl+Shift adds the range to what is selected", () => {
    const { result } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(5, click));
    act(() => result.current.select(1, ctrl));
    act(() => result.current.select(3, ctrlShift));
    expect(ids(result)).toEqual([1, 2, 3, 5]);
  });

  test("a Ctrl-click moves the anchor too", () => {
    const { result } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(1, click));
    act(() => result.current.select(4, ctrl));
    act(() => result.current.select(5, shift));
    expect(ids(result)).toEqual([4, 5]);
  });

  test("Shift-click with no anchor is an ordinary click and becomes the anchor", () => {
    const { result } = setup([1, 2, 3, 4]);
    act(() => result.current.select(2, shift));
    expect(ids(result)).toEqual([2]);
    act(() => result.current.select(4, shift));
    expect(ids(result)).toEqual([2, 3, 4]);
  });

  test("the range follows the order as it is now, not as it was when the anchor was set", () => {
    const { result, rerender } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(1, click));
    // A filter drops 2 and 3 from the screen.
    rerender({ order: [1, 4, 5] });
    act(() => result.current.select(5, shift));
    expect(ids(result)).toEqual([1, 4, 5]);
    // A sort reverses it: the range is the anchor and everything on the way.
    rerender({ order: [5, 4, 1] });
    act(() => result.current.select(4, shift));
    expect(ids(result)).toEqual([1, 4]);
  });

  test("an anchor that has left the order makes Shift an ordinary click", () => {
    const { result, rerender } = setup([1, 2, 3, 4]);
    act(() => result.current.select(1, click));
    rerender({ order: [2, 3, 4] });
    act(() => result.current.select(3, shift));
    expect(ids(result)).toEqual([3]);
    act(() => result.current.select(4, shift));
    expect(ids(result)).toEqual([3, 4]);
  });

  test("Ctrl+Shift with an anchor that has left the order toggles the row", () => {
    const { result, rerender } = setup([1, 2, 3]);
    act(() => result.current.select(1, click));
    rerender({ order: [2, 3] });
    act(() => result.current.select(2, ctrlShift));
    expect(ids(result)).toEqual([1, 2]);
  });

  test("toggleGroup selects the group, then clears it, and anchors on its first row", () => {
    const { result } = setup([1, 2, 3, 4, 5]);
    act(() => result.current.select(5, click));
    act(() => result.current.toggleGroup([2, 3]));
    expect(ids(result)).toEqual([2, 3, 5]);
    act(() => result.current.select(4, shift));
    expect(ids(result)).toEqual([2, 3, 4]);
    act(() => result.current.toggleGroup([2, 3]));
    expect(ids(result)).toEqual([4]);
  });

  test("toggleGroup of nothing changes nothing", () => {
    const { result } = setup([1, 2]);
    act(() => result.current.select(1, click));
    act(() => result.current.toggleGroup([]));
    expect(ids(result)).toEqual([1]);
  });

  test("selectOnly replaces the selection and anchors on the first row", () => {
    const { result } = setup([1, 2, 3, 4]);
    act(() => result.current.select(4, click));
    act(() => result.current.selectOnly([1, 2, 3]));
    expect(ids(result)).toEqual([1, 2, 3]);
    act(() => result.current.select(2, shift));
    expect(ids(result)).toEqual([1, 2]);
  });

  test("extendTo anchors on the row the key was pressed in when there is no anchor", () => {
    const { result } = setup([1, 2, 3, 4]);
    act(() => result.current.extendTo(2, 3));
    expect(ids(result)).toEqual([2, 3]);
    act(() => result.current.extendTo(3, 4));
    expect(ids(result)).toEqual([2, 3, 4]);
  });

  test("setAnchor(null) forgets the anchor", () => {
    const { result } = setup([1, 2, 3]);
    act(() => result.current.select(1, click));
    act(() => result.current.setAnchor(null));
    act(() => result.current.select(3, shift));
    expect(ids(result)).toEqual([3]);
  });

  test("the handlers keep one identity across renders", () => {
    const { result, rerender } = setup([1, 2, 3]);
    const before = result.current;
    rerender({ order: [3, 2, 1] });
    expect(result.current.select).toBe(before.select);
    expect(result.current.toggleGroup).toBe(before.toggleGroup);
    expect(result.current.selectOnly).toBe(before.selectOnly);
    expect(result.current.extendTo).toBe(before.extendTo);
  });

  test("a kept anchor is written to and read from the caller's state", () => {
    const { result } = setup([1, 2, 3, 4], {}, true);
    act(() => result.current.select(2, click));
    expect(result.current.anchor).toBe(2);
    act(() => result.current.select(4, shift));
    expect(result.current.anchor).toBe(2);
    expect(ids(result)).toEqual([2, 3, 4]);
  });

  describe("option plainClick: toggle", () => {
    test("a plain click flips the row and keeps the others", () => {
      const { result } = setup([1, 2, 3], { plainClick: "toggle" });
      act(() => result.current.select(1, click));
      act(() => result.current.select(3, click));
      expect(ids(result)).toEqual([1, 3]);
      act(() => result.current.select(1, click));
      expect(ids(result)).toEqual([3]);
    });
  });

  describe("option range: add", () => {
    test("a Shift range is added to what is selected", () => {
      const { result } = setup([1, 2, 3, 4, 5], { plainClick: "toggle", range: "add" });
      act(() => result.current.select(1, click));
      act(() => result.current.select(5, click));
      act(() => result.current.select(3, click));
      expect(ids(result)).toEqual([1, 3, 5]);
      act(() => result.current.select(4, shift));
      expect(ids(result)).toEqual([1, 3, 4, 5]);
    });

    test("the default range replaces the selection", () => {
      const { result } = setup([1, 2, 3, 4, 5]);
      act(() => result.current.select(1, click));
      act(() => result.current.select(5, ctrl));
      act(() => result.current.select(3, ctrl));
      act(() => result.current.select(3, shift));
      expect(ids(result)).toEqual([3]);
    });
  });
});
