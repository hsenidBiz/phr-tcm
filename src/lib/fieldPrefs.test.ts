import { afterEach, expect, test } from "vitest";
import { autoPick, loadFieldPrefs, saveFieldPrefs } from "./fieldPrefs";
import { setTourRunning } from "../tour/tourState";

afterEach(() => {
  localStorage.clear();
  // A test that forgets to flip this back off must not leak a running
  // tour into whatever runs next in this file.
  setTourRunning(false);
});

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

// The tour shows a made-up project - whatever it auto-picks for Module and
// Preconditions must not sit on disk under that project's name once the
// tour is gone.
test("a running tour writes no field prefs", () => {
  setTourRunning(true);
  saveFieldPrefs("Northwind", "Website", { moduleRef: "Custom.M", preconditionsRef: null });
  expect(loadFieldPrefs("Northwind", "Website")).toBeNull();

  setTourRunning(false);
  saveFieldPrefs("Northwind", "Website", { moduleRef: "Custom.M", preconditionsRef: null });
  expect(loadFieldPrefs("Northwind", "Website")).toEqual({
    moduleRef: "Custom.M",
    preconditionsRef: null,
  });
});
