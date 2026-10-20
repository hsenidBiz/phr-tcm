import { expect, test } from "vitest";
import { inlineWordDiff } from "./wordDiff";

test("inline diff: insertion yields plain text with one green segment", () => {
  const d = inlineWordDiff(
    'Click the card on the "Create Cycle" page (Step 1 of 9).',
    'Click the card on the "Create Cycle" page in PMS Module (Step 1 of 9).',
  );
  expect(d).toEqual([
    { text: 'Click the card on the "Create Cycle" page', kind: "same" },
    { text: "in PMS Module", kind: "added" },
    { text: "(Step 1 of 9).", kind: "same" },
  ]);
});

test("inline diff: a whitespace-only edit still reads as changed", () => {
  const d = inlineWordDiff("Open the login page", "Open the\nlogin  page");
  expect(d.some((s) => s.kind !== "same")).toBe(true);
});

test("inline diff: replacement shows the deletion struck in place", () => {
  const d = inlineWordDiff("Open the login page", "Open the dashboard page");
  expect(d).toEqual([
    { text: "Open the", kind: "same" },
    { text: "dashboard", kind: "added" },
    { text: "login", kind: "removed" },
    { text: "page", kind: "same" },
  ]);
});
