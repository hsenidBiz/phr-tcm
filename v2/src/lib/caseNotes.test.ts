import { afterEach, expect, test } from "vitest";
import { loadNotes, saveNote } from "./caseNotes";

afterEach(() => localStorage.clear());

test("notes round-trip per org and case", () => {
  expect(loadNotes("acme")).toEqual({});
  saveNote("acme", 201, "Steps 3-4 need the new dialog flow");
  expect(loadNotes("acme")["201"]).toBe("Steps 3-4 need the new dialog flow");
  // Another org is a separate namespace.
  expect(loadNotes("other")).toEqual({});
});

test("an empty note removes the entry", () => {
  saveNote("acme", 201, "temp");
  saveNote("acme", 201, "   ");
  expect(loadNotes("acme")).toEqual({});
});

test("corrupt storage falls back to empty", () => {
  localStorage.setItem("tcm-v2-case-notes:acme", "not json");
  expect(loadNotes("acme")).toEqual({});
});
