import { describe, expect, test } from "vitest";
import { moveItem, sameOrder } from "./suiteOrder";

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
