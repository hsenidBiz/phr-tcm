import { expect, test } from "vitest";
import { plural } from "./plural";

test("one is singular; zero and many take the plural", () => {
  expect(plural(1, "case")).toBe("1 case");
  expect(plural(0, "case")).toBe("0 cases");
  expect(plural(3, "case")).toBe("3 cases");
});

test("an irregular plural is passed in", () => {
  expect(plural(1, "entry", "entries")).toBe("1 entry");
  expect(plural(2, "entry", "entries")).toBe("2 entries");
});
