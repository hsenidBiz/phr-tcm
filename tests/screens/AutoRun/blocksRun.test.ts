import { expect, test } from "vitest";
import { SIMPLE_HIDDEN, blockingRows, fixtureBlock, shownInSimple, type SetupRowName } from "../../../src/screens/AutoRun/blocksRun";

const QUIET = { signIn: "saved" as const, discovering: false, fixturesFailed: 0, fixturesFailedHere: 0 };

const ALL: SetupRowName[] = [
  "Environment",
  "Site address",
  "Sign-in",
  "Accounts",
  "Areas",
  "Discovery",
  "Components",
  "Test files",
  "Fixtures",
  "Database",
];

test("nothing blocks a quiet setup", () => {
  expect(blockingRows(QUIET).size).toBe(0);
  expect(blockingRows({ ...QUIET, signIn: "builtin" }).size).toBe(0);
  expect(blockingRows({ ...QUIET, signIn: null }).size).toBe(0);
});

test("a recipe that could not be read blocks the Sign-in row", () => {
  expect([...blockingRows({ ...QUIET, signIn: null, recipeUnreadable: true })]).toEqual(["Sign-in"]);
});

test("a discovery holding the browser blocks the Discovery row", () => {
  expect([...blockingRows({ ...QUIET, discovering: true })]).toEqual(["Discovery"]);
});

test("a failed fixture, either count, blocks the Fixtures row", () => {
  expect([...blockingRows({ ...QUIET, fixturesFailed: 1 })]).toEqual(["Fixtures"]);
  expect([...blockingRows({ ...QUIET, fixturesFailedHere: 2 })]).toEqual(["Fixtures"]);
});

test("no sign-in blocks the Sign-in row", () => {
  expect([...blockingRows({ ...QUIET, signIn: "none" })]).toEqual(["Sign-in"]);
});

test("several blockers add up", () => {
  const b = blockingRows({ signIn: "none", discovering: true, fixturesFailed: 1, fixturesFailedHere: 0 });
  expect([...b].sort()).toEqual(["Discovery", "Fixtures", "Sign-in"]);
});

test("Simple shows every row but the hidden three when nothing blocks", () => {
  for (const row of ALL) expect(shownInSimple(row, new Set())).toBe(!SIMPLE_HIDDEN.has(row));
  expect([...SIMPLE_HIDDEN].sort()).toEqual(["Components", "Discovery", "Fixtures"]);
});

test("a blocking hidden row is shown, and only that one", () => {
  const blocking = new Set<SetupRowName>(["Discovery"]);
  for (const row of ALL) {
    expect(shownInSimple(row, blocking)).toBe(row === "Discovery" || !SIMPLE_HIDDEN.has(row));
  }
});

test("Simple never shows Fixtures, blocking or not; Discovery and Sign-in still come back", () => {
  const failed = blockingRows({ ...QUIET, fixturesFailed: 1 });
  expect(failed.has("Fixtures")).toBe(true);
  expect(shownInSimple("Fixtures", failed)).toBe(false);
  expect(shownInSimple("Fixtures", new Set())).toBe(false);
  expect(shownInSimple("Discovery", new Set<SetupRowName>(["Discovery"]))).toBe(true);
  expect(shownInSimple("Sign-in", new Set<SetupRowName>(["Sign-in"]))).toBe(true);
});

test("fixtureBlock is true when either count is above zero", () => {
  expect(fixtureBlock(0, 0)).toBe(false);
  expect(fixtureBlock(1, 0)).toBe(true);
  expect(fixtureBlock(0, 1)).toBe(true);
});
