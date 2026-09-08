// Per-project default tags for Manual Entry.

import { afterEach, expect, test } from "vitest";
import { loadDefaultTags, saveDefaultTags } from "./defaultTags";

afterEach(() => localStorage.clear());

test("round-trips per org/project, isolated between projects", () => {
  saveDefaultTags("acme", "Web", "smoke; HRM");
  saveDefaultTags("acme", "Mobile", "sanity");
  expect(loadDefaultTags("acme", "Web")).toBe("smoke; HRM");
  expect(loadDefaultTags("acme", "Mobile")).toBe("sanity");
  expect(loadDefaultTags("other", "Web")).toBe("");
});

test("saving empty clears the stored key entirely", () => {
  saveDefaultTags("acme", "Web", "smoke");
  saveDefaultTags("acme", "Web", "   ");
  expect(localStorage.getItem("tcm-v2-default-tags:acme/Web")).toBeNull();
  expect(loadDefaultTags("acme", "Web")).toBe("");
});

test("a missing scope never reads or writes", () => {
  saveDefaultTags("", "", "smoke");
  expect(loadDefaultTags("", "")).toBe("");
});
