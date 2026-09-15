import { describe, expect, test } from "vitest";
import {
  groupKeys, moveBlock, moveBlockBefore, moveItem, nudgeBlock, orderByGroups, orderFromFiles, orderGroupsAZ,
  sameOrder, sectionsOf, type SuiteCase,
} from "./suiteOrder";

const c = (id: number, title = `Case ${id}`): SuiteCase => ({ id, title });
const ids = (l: SuiteCase[]) => l.map((x) => x.id);

describe("moveItem", () => {
  test("moves an item to a new index and returns a new array", () => {
    const list = ["a", "b", "c", "d"];
    expect(moveItem(list, 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveItem(list, 3, 0)).toEqual(["d", "a", "b", "c"]);
    expect(list).toEqual(["a", "b", "c", "d"]);
  });
  test("out-of-range or same index leaves the order alone", () => {
    const list = ["a", "b"];
    expect(moveItem(list, 1, 1)).toEqual(["a", "b"]);
    expect(moveItem(list, 5, 0)).toEqual(["a", "b"]);
    expect(moveItem(list, 0, -1)).toEqual(["a", "b"]);
  });
});

describe("sameOrder", () => {
  test("compares by id sequence only", () => {
    const a = [{ id: 1, title: "x" }, { id: 2, title: "y" }];
    expect(sameOrder(a, [{ id: 1, title: "other" }, { id: 2, title: "y" }])).toBe(true);
    expect(sameOrder(a, [{ id: 2, title: "y" }, { id: 1, title: "x" }])).toBe(false);
    expect(sameOrder(a, [{ id: 1, title: "x" }])).toBe(false);
  });
});

describe("moveBlock", () => {
  const list = [c(1), c(2), c(3), c(4), c(5)];
  test("a block dropped below lands after the target, keeping its own order", () => {
    expect(ids(moveBlock(list, new Set([1, 2]), 4))).toEqual([3, 4, 1, 2, 5]);
  });
  test("a block dropped above lands before the target", () => {
    expect(ids(moveBlock(list, new Set([4, 5]), 2))).toEqual([1, 4, 5, 2, 3]);
  });
  test("a scattered selection is gathered into one block at the drop", () => {
    expect(ids(moveBlock(list, new Set([1, 3]), 5))).toEqual([2, 4, 5, 1, 3]);
  });
  test("dropping onto a member of the block, or an unknown target, changes nothing", () => {
    expect(ids(moveBlock(list, new Set([2, 3]), 3))).toEqual([1, 2, 3, 4, 5]);
    expect(ids(moveBlock(list, new Set([2]), 99))).toEqual([1, 2, 3, 4, 5]);
    expect(moveBlock(list, new Set([2]), 99)).not.toBe(list);
  });
  test("one id behaves exactly like moveItem", () => {
    expect(ids(moveBlock(list, new Set([1]), 3))).toEqual(ids(moveItem(list, 0, 2)));
    expect(ids(moveBlock(list, new Set([4]), 2))).toEqual(ids(moveItem(list, 3, 1)));
  });
});

describe("moveBlockBefore", () => {
  const list = [c(1), c(2), c(3), c(4), c(5)];
  test("a block from ABOVE the target still lands before it, not after (moveBlock would put it after)", () => {
    expect(ids(moveBlockBefore(list, new Set([1, 2]), 4))).toEqual([3, 1, 2, 4, 5]);
  });
  test("a block from BELOW the target lands before it too, same as moveBlock", () => {
    expect(ids(moveBlockBefore(list, new Set([4, 5]), 2))).toEqual([1, 4, 5, 2, 3]);
  });
  test("dropping onto a member of the block, or an unknown target, changes nothing", () => {
    expect(ids(moveBlockBefore(list, new Set([2, 3]), 3))).toEqual([1, 2, 3, 4, 5]);
    expect(ids(moveBlockBefore(list, new Set([2]), 99))).toEqual([1, 2, 3, 4, 5]);
    expect(moveBlockBefore(list, new Set([2]), 99)).not.toBe(list);
  });
});

describe("nudgeBlock", () => {
  const list = [c(1), c(2), c(3), c(4), c(5)];
  test("moves the block one step, gathering a scattered selection first", () => {
    expect(ids(nudgeBlock(list, new Set([3, 4]), "up"))).toEqual([1, 3, 4, 2, 5]);
    expect(ids(nudgeBlock(list, new Set([3, 4]), "down"))).toEqual([1, 2, 5, 3, 4]);
    expect(ids(nudgeBlock(list, new Set([2, 4]), "up"))).toEqual([2, 4, 1, 3, 5]);
  });
  test("stops at the ends", () => {
    expect(ids(nudgeBlock(list, new Set([1, 2]), "up"))).toEqual([1, 2, 3, 4, 5]);
    expect(ids(nudgeBlock(list, new Set([4, 5]), "down"))).toEqual([1, 2, 3, 4, 5]);
  });
});

describe("groups", () => {
  // Title groups as the rest of the app sees them: the text before the
  // first separator, and only when at least two titles share it.
  const list = [
    c(1, "Alerts | ring once"),
    c(2, "Filter Card | collapses"),
    c(3, "Alerts | ring twice"),
    c(4, "Just one of these"),
    c(5, "Filter Card | expands"),
  ];
  test("groupKeys names each case's group, blank for the ungrouped", () => {
    expect(groupKeys(list)).toEqual(["Alerts", "Filter Card", "Alerts", "", "Filter Card"]);
  });
  test("sectionsOf reads the CURRENT order as runs, so a split group is two sections", () => {
    expect(sectionsOf(list)).toEqual([
      { name: "Alerts", ids: [1] },
      { name: "Filter Card", ids: [2] },
      { name: "Alerts", ids: [3] },
      { name: "", ids: [4] },
      { name: "Filter Card", ids: [5] },
    ]);
  });
  test("sectionsOf merges adjacent cases of one group, and adjacent ungrouped cases, into one section", () => {
    const l = [
      c(1, "Alerts | ring once"),
      c(2, "Alerts | ring twice"),
      c(3, "Just one"),
      c(4, "Another single"),
      c(5, "Filter Card | a"),
      c(6, "Filter Card | b"),
      c(7, "Alerts | ring thrice"),
    ];
    expect(sectionsOf(l)).toEqual([
      { name: "Alerts", ids: [1, 2] },
      { name: "", ids: [3, 4] },
      { name: "Filter Card", ids: [5, 6] },
      { name: "Alerts", ids: [7] },
    ]);
  });
  test("orderByGroups makes each group contiguous in order of first appearance, ungrouped last", () => {
    expect(ids(orderByGroups(list))).toEqual([1, 3, 2, 5, 4]);
    // Already arranged: same order back.
    expect(ids(orderByGroups(orderByGroups(list)))).toEqual([1, 3, 2, 5, 4]);
  });
  test("orderGroupsAZ sorts the groups by name, case-insensitively, cases keeping their order", () => {
    const l = [c(1, "zeta | a"), c(2, "Alpha | a"), c(3, "zeta | b"), c(4, "alpha | b"), c(5, "solo")];
    expect(ids(orderGroupsAZ(l))).toEqual([2, 4, 1, 3, 5]);
  });
});

describe("orderFromFiles", () => {
  const suite = [c(1), c(2), c(3), c(4), c(5), c(6)];
  const file = (name: string, ...update_ids: Array<number | null>) => ({
    name,
    cases: update_ids.map((update_id) => ({ update_id })),
  });
  test("each file is a block in the file's ROW order - tester_order plays no part", () => {
    const { order, placed } = orderFromFiles(suite, [file("a.json", 3, 1)]);
    expect(ids(order)).toEqual([3, 1, 2, 4, 5, 6]);
    expect(placed).toEqual([2]);
  });
  test("blocks follow the files' order, and cases in no file trail in their current order", () => {
    const { order, placed } = orderFromFiles(suite, [file("b.json", 6, 5), file("a.json", 2)]);
    expect(ids(order)).toEqual([6, 5, 2, 1, 3, 4]);
    expect(placed).toEqual([2, 1]);
  });
  test("a case in two files goes with the first, and is counted", () => {
    const { order, placed, duplicates } = orderFromFiles(suite, [file("a.json", 1, 2), file("b.json", 2, 3)]);
    expect(ids(order)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(placed).toEqual([2, 1]);
    expect(duplicates).toBe(1);
  });
  test("a case in three files still counts as one duplicate, not two", () => {
    const { duplicates } = orderFromFiles(suite, [file("a.json", 1), file("b.json", 1), file("c.json", 1)]);
    expect(duplicates).toBe(1);
  });
  test("ids not in this suite, null ids and repeats inside one file are ignored", () => {
    const { order, placed, duplicates } = orderFromFiles(suite, [file("a.json", 99, null, 4, 4)]);
    expect(ids(order)).toEqual([4, 1, 2, 3, 5, 6]);
    expect(placed).toEqual([1]);
    expect(duplicates).toBe(0);
  });
  test("no files, or files placing nothing, return the current order and zero counts", () => {
    expect(ids(orderFromFiles(suite, []).order)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(orderFromFiles(suite, [file("x.json", 99)]).placed).toEqual([0]);
  });
});
