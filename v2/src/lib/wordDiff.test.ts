import { expect, test } from "vitest";
import { wordDiff } from "./wordDiff";

test("an insertion highlights only the added words", () => {
  const d = wordDiff(
    'Click the card on the "Create Cycle" page (Step 1 of 9).',
    'Click the card on the "Create Cycle" page in PMS Module (Step 1 of 9).',
  );
  // Old side: nothing was removed.
  expect(d.old.every((t) => !t.changed)).toBe(true);
  // New side: exactly the inserted phrase is marked.
  expect(d.new.filter((t) => t.changed).map((t) => t.text)).toEqual(["in PMS Module"]);
});

test("a replacement marks both sides' differing words", () => {
  const d = wordDiff("Open the login page", "Open the dashboard page");
  expect(d.old.filter((t) => t.changed).map((t) => t.text)).toEqual(["login"]);
  expect(d.new.filter((t) => t.changed).map((t) => t.text)).toEqual(["dashboard"]);
});

test("completely different texts mark everything", () => {
  const d = wordDiff("alpha beta", "gamma delta");
  expect(d.old).toEqual([{ text: "alpha beta", changed: true }]);
  expect(d.new).toEqual([{ text: "gamma delta", changed: true }]);
});

test("adjacent same-flag words merge into one token", () => {
  const d = wordDiff("a b c", "a x y c");
  expect(d.new).toEqual([
    { text: "a", changed: false },
    { text: "x y", changed: true },
    { text: "c", changed: false },
  ]);
});
