import { describe, expect, test } from "vitest";
import { moveItem, orderFromFile, sameOrder } from "./suiteOrder";

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

describe("orderFromFile", () => {
  const suite = [
    { id: 201, title: "a" },
    { id: 202, title: "b" },
    { id: 203, title: "c" },
    { id: 204, title: "d" },
  ];
  test("sorts the suite's cases by the file's tester_order and appends the rest", () => {
    const { order, matched } = orderFromFile(suite, [
      { update_id: 203, tester_order: 1 },
      { update_id: 201, tester_order: 3 },
      { update_id: 202, tester_order: 2 },
      { update_id: 999, tester_order: 0 }, // not in the suite
      { update_id: null, tester_order: 4 }, // never uploaded
      { update_id: 204, tester_order: null }, // in the suite, no order
    ]);
    expect(order.map((c) => c.id)).toEqual([203, 202, 201, 204]);
    expect(matched).toBe(3);
  });
  test("ties keep file position; a file with no orders matches nothing", () => {
    const tie = orderFromFile(suite, [
      { update_id: 202, tester_order: 1 },
      { update_id: 201, tester_order: 1 },
    ]);
    expect(tie.order.map((c) => c.id)).toEqual([202, 201, 203, 204]);
    expect(orderFromFile(suite, [{ update_id: 201 }]).matched).toBe(0);
  });
});
