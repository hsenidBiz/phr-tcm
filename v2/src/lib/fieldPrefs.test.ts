import { afterEach, expect, test } from "vitest";
import { autoPick, loadFieldPrefs, saveFieldPrefs } from "./fieldPrefs";

afterEach(() => localStorage.clear());

const fields = [
  { name: "Apples", reference_name: "Custom.Apples" },
  { name: "Feature Module", reference_name: "Custom.Module" },
  { name: "Prerequisites", reference_name: "Custom.Prereq" },
];

test("auto-pick matches v1's name-contains rule", () => {
  const p = autoPick(fields);
  expect(p.moduleRef).toBe("Custom.Module");
  expect(p.preconditionsRef).toBe("Custom.Prereq");
});

test("auto-pick prefers an exact 'Module' over Sub Module / Module Group", () => {
  const p = autoPick([
    { name: "Sub Module", reference_name: "Custom.SubModule" },
    { name: "Module", reference_name: "Custom.Module" },
    { name: "Module Group", reference_name: "Custom.ModuleGroup" },
  ]);
  expect(p.moduleRef).toBe("Custom.Module");

  // Without an exact match, a name starting with the term wins.
  const p2 = autoPick([
    { name: "Sub Module", reference_name: "Custom.SubModule" },
    { name: "Module Group", reference_name: "Custom.ModuleGroup" },
  ]);
  expect(p2.moduleRef).toBe("Custom.ModuleGroup");
});

test("auto-pick skips when nothing matches", () => {
  const p = autoPick([{ name: "Apples", reference_name: "Custom.Apples" }]);
  expect(p.moduleRef).toBeNull();
  expect(p.preconditionsRef).toBeNull();
});

test("prefs persist per org/project", () => {
  saveFieldPrefs("acme", "Web", { moduleRef: "Custom.M", preconditionsRef: null });
  expect(loadFieldPrefs("acme", "Web")).toEqual({
    moduleRef: "Custom.M",
    preconditionsRef: null,
  });
  expect(loadFieldPrefs("acme", "Other")).toBeNull();
});
