import { expect, test } from "vitest";
import type { TestCase } from "../bindings";
import { buildTestMap, splitArea, UNGROUPED } from "./testMap";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [{ action: "Open", expected: "Shown" }],
  tags: "smoke",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "Signed in",
  update_id: null,
  ...over,
});

const names = (nodes: ReturnType<typeof buildTestMap>) => nodes.map((n) => n.name);

test("an area path splits on the slash, spaces optional, blanks dropped", () => {
  expect(splitArea("Manage Events / Create / Validation")).toEqual(["Manage Events", "Create", "Validation"]);
  expect(splitArea("Manage Events/Create")).toEqual(["Manage Events", "Create"]);
  expect(splitArea(" / Page // ")).toEqual(["Page"]);
  expect(splitArea("")).toEqual([]);
  expect(splitArea(undefined)).toEqual([]);
});

test("cases with an area land under their path, with counts up the tree", () => {
  const map = buildTestMap([
    tc("Grid shows columns", { area: "Manage Events / Grid", update_id: 81310 }),
    tc("Create validates dates", { area: "Manage Events / Create / Validation" }),
    tc("Create saves", { area: "Manage Events / Create" }),
    tc("Page opens", { area: "Manage Events" }),
  ]);
  expect(names(map)).toEqual(["Manage Events"]);
  const root = map[0];
  expect(root.count).toBe(4);
  expect(root.cases.map((c) => c.title)).toEqual(["Page opens"]);
  expect(names(root.children)).toEqual(["Create", "Grid"]); // A-Z
  const create = root.children[0];
  expect(create.count).toBe(2);
  expect(create.cases.map((c) => c.title)).toEqual(["Create saves"]);
  expect(create.children[0].name).toBe("Validation");
  expect(create.children[0].count).toBe(1);
  // A case carries what the side panel shows, and its id (or null).
  expect(root.children[1].cases[0]).toEqual({
    id: 81310,
    title: "Grid shows columns",
    steps: [{ action: "Open", expected: "Shown" }],
    preconditions: "Signed in",
    tags: "smoke",
    automation_status: "Not Automated",
  });
});

test("cases without an area fall back to the title grouping, Ungrouped last", () => {
  const map = buildTestMap([
    tc("Login | valid credentials"),
    tc("Login | locked out"),
    tc("Something on its own"),
  ]);
  expect(names(map)).toEqual(["Login", UNGROUPED]);
  expect(map[0].count).toBe(2);
  expect(map[1].cases[0].title).toBe("Something on its own");
});

test("a title group joins an area node of the same name, whatever the case", () => {
  const map = buildTestMap([
    tc("Grid shows columns", { area: "manage events / Grid" }),
    tc("Manage Events | page navigation"),
    tc("Manage Events | page validation"),
  ]);
  expect(names(map)).toEqual(["manage events"]); // first spelling seen is kept
  expect(map[0].count).toBe(3);
  expect(map[0].cases.map((c) => c.title)).toEqual([
    "Manage Events | page navigation",
    "Manage Events | page validation",
  ]);
  expect(names(map[0].children)).toEqual(["Grid"]);
});

test("an empty set maps to nothing", () => {
  expect(buildTestMap([])).toEqual([]);
});
