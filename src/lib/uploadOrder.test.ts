import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import { orderForUpload } from "./uploadOrder";

const tc = (title: string, tester_order: number | null): TestCase => ({
  title,
  steps: [],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  spec_order: null,
  tester_order,
});

/// Upload order is suite order, so a fully optimized queue goes out the
/// way the run sheet reads, whatever order the screen happened to be in.
test("a fully ordered list is sent in tester order", () => {
  const out = orderForUpload([tc("C", 3), tc("A", 1), tc("B", 2)]);
  expect(out.map((c) => c.title)).toEqual(["A", "B", "C"]);
});

test("ties keep their screen order", () => {
  const out = orderForUpload([tc("X", 2), tc("Y", 1), tc("Z", 2)]);
  expect(out.map((c) => c.title)).toEqual(["Y", "X", "Z"]);
});

/// Half an order is no order: one case without a rank leaves the screen
/// order alone rather than interleaving ranked and unranked cases.
test("a list with any unranked case is left as it is", () => {
  const list = [tc("C", 3), tc("A", null), tc("B", 2)];
  expect(orderForUpload(list)).toBe(list);
});

test("one case or none is returned untouched", () => {
  const one = [tc("A", 5)];
  expect(orderForUpload(one)).toBe(one);
  expect(orderForUpload([])).toEqual([]);
});
