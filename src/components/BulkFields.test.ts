import { expect, test } from "vitest";
import { NO_BULK_CHANGE, bulkChangedFields, bulkModule, bulkTags } from "./BulkFields";

test("a module of only spaces is no change, and a module is trimmed", () => {
  expect(bulkChangedFields({ ...NO_BULK_CHANGE, moduleValue: "  " })).toEqual([]);
  expect(bulkModule({ ...NO_BULK_CHANGE, moduleValue: "  " }, "Payroll")).toBe("Payroll");
  expect(bulkChangedFields({ ...NO_BULK_CHANGE, moduleValue: " Billing " })).toEqual(["Module"]);
  expect(bulkModule({ ...NO_BULK_CHANGE, moduleValue: " Billing " }, "Payroll")).toBe("Billing");
});

test("clearing the module sends an empty module whatever the case had", () => {
  const v = { ...NO_BULK_CHANGE, moduleClear: true };
  expect(bulkChangedFields(v)).toEqual(["Module (clear)"]);
  expect(bulkModule(v, "Payroll")).toBe("");
});

test("Add tags counts only with a tag typed, and leaves the tags alone otherwise", () => {
  const add = { ...NO_BULK_CHANGE, tagsMode: "add" as const };
  expect(bulkChangedFields(add)).toEqual([]);
  expect(bulkChangedFields({ ...add, tags: " ; " })).toEqual([]);
  expect(bulkTags(add, "a;b")).toBe("a;b");
  expect(bulkChangedFields({ ...add, tags: "c" })).toEqual(["Tags"]);
  expect(bulkTags({ ...add, tags: "c" }, "a;b")).toBe("a; b; c");
});

test("Replace tags with nothing is a clear, and with tags is a change", () => {
  const replace = { ...NO_BULK_CHANGE, tagsMode: "replace" as const };
  expect(bulkChangedFields(replace)).toEqual(["Tags (clear)"]);
  expect(bulkChangedFields({ ...replace, tags: "   " })).toEqual(["Tags (clear)"]);
  expect(bulkTags(replace, "a;b")).toBe("");
  expect(bulkChangedFields({ ...replace, tags: "x" })).toEqual(["Tags"]);
});
