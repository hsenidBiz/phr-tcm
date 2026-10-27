import { afterEach, expect, test } from "vitest";
import { loadPinnedAreas, pinnedAreasKey, savePinnedAreas, seedCombinedPins, togglePin } from "./boardPins";
import { COMBINED } from "./projectScope";

afterEach(() => localStorage.clear());

test("pinned areas are kept per org/project, in the order they were pinned", () => {
  savePinnedAreas("acme", "Web", ["HRM\\Gamma", "HRM\\Alpha"]);
  expect(loadPinnedAreas("acme", "Web")).toEqual(["HRM\\Gamma", "HRM\\Alpha"]);
  expect(loadPinnedAreas("acme", "Mobile")).toEqual([]);
  expect(localStorage.getItem(pinnedAreasKey("acme", "Web"))).toBe('["HRM\\\\Gamma","HRM\\\\Alpha"]');
});

test("togglePin adds at the end and removes in place", () => {
  expect(togglePin([], "A")).toEqual(["A"]);
  expect(togglePin(["A"], "B")).toEqual(["A", "B"]);
  expect(togglePin(["A", "B", "C"], "B")).toEqual(["A", "C"]);
});

test("anything unreadable is ignored rather than trusted", () => {
  localStorage.setItem(pinnedAreasKey("acme", "Web"), "not json");
  expect(loadPinnedAreas("acme", "Web")).toEqual([]);
  localStorage.setItem(pinnedAreasKey("acme", "Web"), '{"a":1}');
  expect(loadPinnedAreas("acme", "Web")).toEqual([]);
  localStorage.setItem(pinnedAreasKey("acme", "Web"), '["A", 3, "", "A", null, "B"]');
  expect(loadPinnedAreas("acme", "Web")).toEqual(["A", "B"]);
});

test("combined pins seed once from the home project and keep it", () => {
  const home = '["HRM\\\\Gamma","HRM\\\\Alpha"]';
  savePinnedAreas("acme", "HRM", ["HRM\\Gamma", "HRM\\Alpha"]);

  seedCombinedPins("acme", "HRM");
  expect(loadPinnedAreas("acme", COMBINED)).toEqual(["HRM\\Gamma", "HRM\\Alpha"]);
  expect(localStorage.getItem(pinnedAreasKey("acme", COMBINED))).toBe(home);
  // Copied, never moved: switching the projects apart again finds them.
  expect(localStorage.getItem(pinnedAreasKey("acme", "HRM"))).toBe(home);

  // An edit while combined is not undone by the next seed, and the home
  // project's own pins stay as they were.
  savePinnedAreas("acme", COMBINED, ["HRM\\Alpha"]);
  seedCombinedPins("acme", "HRM");
  expect(loadPinnedAreas("acme", COMBINED)).toEqual(["HRM\\Alpha"]);
  expect(localStorage.getItem(pinnedAreasKey("acme", "HRM"))).toBe(home);
});

test("combined pins seed nothing when the home project pinned nothing", () => {
  seedCombinedPins("acme", "HRM");
  expect(localStorage.getItem(pinnedAreasKey("acme", COMBINED))).toBeNull();
  seedCombinedPins("acme", "");
  expect(localStorage.getItem(pinnedAreasKey("acme", COMBINED))).toBeNull();
});
