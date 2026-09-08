// Port of v1 tests/test_grouping.py - same golden expectations.
import { expect, test } from "vitest";
import { groupIndices } from "./grouping";

function named(result: ReturnType<typeof groupIndices>) {
  return Object.fromEntries(result.map((g) => [g.name, g.indices]));
}

test("delimiter prefix groups shared category", () => {
  const groups = named(
    groupIndices(["Login - valid credentials", "Login - locked out", "Checkout: empty cart"]),
  );
  expect(groups["Login"]).toEqual([0, 1]);
  expect(groups["Checkout"]).toBeUndefined(); // alone -> ungrouped
  expect(groups[""]).toEqual([2]);
});

test("various delimiters recognised", () => {
  const groups = named(
    groupIndices(["Auth | sign in", "Auth | sign out", "Billing / invoice", "Billing / refund"]),
  );
  expect(groups["Auth"]).toEqual([0, 1]);
  expect(groups["Billing"]).toEqual([2, 3]);
  expect(groups[""]).toBeUndefined();
});

test("earliest delimiter wins", () => {
  const groups = named(groupIndices(["Login - step: one", "Login - step: two"]));
  expect(groups["Login"]).toEqual([0, 1]);
});

test("word prefix fallback when no delimiter", () => {
  const groups = named(
    groupIndices(["User can login", "User can logout", "Admin dashboard loads"]),
  );
  expect(groups["User can"]).toEqual([0, 1]);
  expect(groups[""]).toEqual([2]);
});

test("first-word bucket named by common prefix", () => {
  const groups = named(groupIndices(["Verify homepage loads", "Verify checkout works"]));
  expect(groups["Verify"]).toEqual([0, 1]);
});

test("case-insensitive delimiter grouping", () => {
  const result = groupIndices(["LOGIN - a", "login - b", "Login - c"]);
  expect(result.map((g) => g.name)).toEqual(["LOGIN"]);
  expect(result[0].indices).toEqual([0, 1, 2]);
});

test("bare hyphen is not a delimiter", () => {
  const groups = named(groupIndices(["sign-in works", "sign-out works"]));
  expect(groups[""]).toEqual([0, 1]);
});

test("every index present exactly once", () => {
  const titles = [
    "Login - a",
    "Login - b",
    "Checkout: x",
    "Checkout: y",
    "Standalone title",
    "User can do a",
    "User can do b",
  ];
  const seen = groupIndices(titles)
    .flatMap((g) => g.indices)
    .sort((a, b) => a - b);
  expect(seen).toEqual([0, 1, 2, 3, 4, 5, 6]);
});

test("groups sorted alphabetically, ungrouped last", () => {
  const names = groupIndices(["Zebra - a", "Zebra - b", "Apple - a", "Apple - b", "lonely"]).map(
    (g) => g.name,
  );
  expect(names).toEqual(["Apple", "Zebra", ""]);
});

test("empty and blank titles", () => {
  const groups = named(groupIndices(["", "   ", "Login - a", "Login - b"]));
  expect(groups["Login"]).toEqual([2, 3]);
  expect(groups[""]).toEqual([0, 1]);
});

test("empty input", () => {
  expect(groupIndices([])).toEqual([]);
});
