import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import type { WatchedFile } from "./fileSync";
import { testerOrderSources } from "./testerOrderStart";

const tc = (title: string, update_id: number | null, tester_order: number | null, area = ""): TestCase =>
  ({
    title,
    steps: [],
    tags: "",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id,
    tester_order,
    area,
  }) as TestCase;

const watch = (path: string, snapshot: TestCase[]): WatchedFile => ({ path, stamp: "s", snapshot });

test("a file whose cases all carry a tester order offers them in that order", () => {
  const [src] = testerOrderSources(
    [watch("C:/w/login.json", [tc("A", 201, 3), tc("B", 202, 1), tc("C", 203, 2)])],
    [201, 202, 203],
  );
  expect(src.path).toBe("C:/w/login.json");
  expect(src.label).toBe("Tester order from login.json");
  expect(src.ids).toEqual([202, 203, 201]);
});

test("a file with any case lacking a tester order is not offered", () => {
  // The same all-or-nothing rule the upload uses: a half-optimized file
  // is not a tester order.
  expect(
    testerOrderSources([watch("C:/w/a.json", [tc("A", 201, 1), tc("B", 202, null)])], [201, 202]),
  ).toEqual([]);
});

test("only cases that are in this suite count, and a file with none of them is not offered", () => {
  const [src] = testerOrderSources(
    [watch("C:/w/a.json", [tc("A", 201, 2), tc("New", null, 1), tc("Elsewhere", 999, 3)])],
    [201, 202],
  );
  expect(src.ids).toEqual([201]);
  expect(testerOrderSources([watch("C:/w/b.json", [tc("X", 999, 1)])], [201])).toEqual([]);
});

test("an empty file is not offered", () => {
  expect(testerOrderSources([watch("C:/w/a.json", [])], [201])).toEqual([]);
});

test("a shared tester order keeps file order, and a repeated id appears once", () => {
  const [src] = testerOrderSources(
    [watch("C:/w/a.json", [tc("A", 202, 1), tc("B", 201, 1), tc("A again", 202, 2)])],
    [201, 202],
  );
  expect(src.ids).toEqual([202, 201]);
});

test("each case's area becomes its group, and a blank area gives none", () => {
  const [src] = testerOrderSources(
    [watch("C:/w/a.json", [tc("A", 201, 1, " HRM / Leave "), tc("B", 202, 2, "  ")])],
    [201, 202],
  );
  expect(src.groups.get(201)).toBe("HRM / Leave");
  expect(src.groups.has(202)).toBe(false);
});

test("several qualifying files each get their own source, told apart by full path when names clash", () => {
  const sources = testerOrderSources(
    [
      watch("C:/w/one/cases.json", [tc("A", 201, 1)]),
      watch("C:/w/two/cases.json", [tc("B", 202, 1)]),
      watch("C:/w/other.json", [tc("C", 203, 1)]),
    ],
    [201, 202, 203],
  );
  expect(sources.map((s) => s.label)).toEqual([
    "Tester order from C:/w/one/cases.json",
    "Tester order from C:/w/two/cases.json",
    "Tester order from other.json",
  ]);
});
