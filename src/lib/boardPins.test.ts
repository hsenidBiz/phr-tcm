import { afterEach, expect, test } from "vitest";
import { loadPinnedAreas, pinnedAreasKey, savePinnedAreas, togglePin } from "./boardPins";

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
