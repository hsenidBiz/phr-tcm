import { describe, expect, test } from "vitest";
import type { SuiteRef } from "../bindings";
import { buildTree, flattenTree, indented } from "./suiteTree";

const s = (id: number, parent_id: number | null, name = `S${id}`): SuiteRef => ({
  id,
  name,
  suite_type: "staticTestSuite",
  requirement_id: null,
  parent_id,
});

describe("buildTree", () => {
  test("nests by parent id; unknown parents become roots", () => {
    const roots = buildTree([s(1, null), s(2, 1), s(3, 2), s(4, 99)]);
    expect(roots.map((r) => r.suite.id)).toEqual([1, 4]);
    expect(roots[0].children[0].suite.id).toBe(2);
    expect(roots[0].children[0].children[0].suite.id).toBe(3);
  });
});

describe("flattenTree", () => {
  test("walks depth-first with the depth of each row", () => {
    const rows = flattenTree(buildTree([s(1, null), s(2, 1), s(3, 2), s(4, null)]));
    expect(rows.map((r) => [r.suite.id, r.depth])).toEqual([
      [1, 0],
      [2, 1],
      [3, 2],
      [4, 0],
    ]);
  });
});

describe("indented", () => {
  test("repeats non-breaking spaces, not ordinary ones", () => {
    const result = indented("Smoke", 2);
    expect(result).toBe("        Smoke");
    expect(result.codePointAt(0)).toBe(0xa0);
  });
});
