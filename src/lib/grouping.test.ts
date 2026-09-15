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

// ---- Bracket tags, and separators that are really sentence text ----------
// Field report 2026-09-15 (View Test Cases, Group by title): 490 cases titled
// "[Floor Plan][Area] Verify ..." landed in one group named "[Floor", and
// cases like "[Floor Plan][Hierarchy] Verify Format > Bring to front" fell
// into Ungrouped because the " > " inside the sentence made a one-off group.

test("leading bracket tags are the group", () => {
  const groups = named(
    groupIndices([
      "[Floor Plan][Navigation] Verify the grid shows its columns",
      "[Floor Plan][Create] Verify a name matching an existing plan",
      "[Floor Plan][Navigation] Verify the grid shows an empty state",
      "[Floor Plan][Create] Verify a one-day range",
    ]),
  );
  expect(groups["[Floor Plan][Navigation]"]).toEqual([0, 2]);
  expect(groups["[Floor Plan][Create]"]).toEqual([1, 3]);
  expect(groups["[Floor"]).toBeUndefined();
});

test("a separator inside the sentence does not override the tags", () => {
  const groups = named(
    groupIndices([
      "[Floor Plan][Hierarchy] Verify Format > Bring to front",
      "[Floor Plan][Hierarchy] Verify Format > Send to back",
      "[Floor Plan][Hierarchy] Verify a cluster: members move together",
    ]),
  );
  expect(groups["[Floor Plan][Hierarchy]"]).toEqual([0, 1, 2]);
  expect(groups[""]).toBeUndefined();
});

test("tags match regardless of case and spacing", () => {
  const groups = named(groupIndices(["[Floor Plan][UI] a", "[floor  plan] [ui] b"]));
  expect(groups["[Floor Plan][UI]"]).toEqual([0, 1]);
});

test("a tag set with one case joins the group for its first tag", () => {
  const groups = named(
    groupIndices([
      "[Floor Plan][Known Gap] Save with skipped fields",
      "[Floor Plan][Export] Verify Floor Actions > Export",
      "[Floor Plan][UI] Verify the footer",
      "[Floor Plan][UI] Verify zoom",
    ]),
  );
  expect(groups["[Floor Plan][UI]"]).toEqual([2, 3]);
  expect(groups["[Floor Plan]"]).toEqual([0, 1]);
  expect(groups[""]).toBeUndefined();
});

test("a separator far into the title is sentence text, not a category", () => {
  const groups = named(
    groupIndices(["Verify Floor Actions > Export works", "Verify the Canvas page shows a grid"]),
  );
  expect(groups["Verify Floor Actions"]).toBeUndefined();
  // Word matching still gets its turn.
  expect(groups["Verify"]).toEqual([0, 1]);
});

test("a separator group of one falls back to word matching", () => {
  const groups = named(
    groupIndices(["Login - valid credentials", "Login screen shows the logo", "Login screen shows help"]),
  );
  // "Login" (separator) is alone, so it joins the word pass with the others.
  expect(groups["Login"]).toEqual([0, 1, 2]);
  expect(groups[""]).toBeUndefined();
});

test("a word never splits a tag in half", () => {
  // Tags that do not lead the title go through word matching, where
  // "Plan][Nav]" used to count as one word and cut the name at "[Floor".
  const groups = named(groupIndices(["Check [Floor Plan][Nav] a", "Check [Floor Plan][Grid] b"]));
  expect(groups["Check [Floor Plan]"]).toEqual([0, 1]);
});
