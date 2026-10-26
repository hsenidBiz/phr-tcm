import { afterEach, expect, test, vi } from "vitest";

import { setTourRunning } from "../tour/tourState";
import {
  ALL_PROJECTS_KEY,
  ALL_PROJECTS_NOTICE_KEY,
  COMBINED,
  allProjectsSnapshot,
  effectiveProject,
  loadAllProjects,
  saveAllProjects,
  scopeSegment,
  seedCombinedKey,
  spansProjects,
  subscribeAllProjects,
  takeAllProjectsNotice,
} from "./projectScope";

afterEach(() => {
  setTourRunning(false);
  vi.restoreAllMocks();
  localStorage.clear();
});

test("the switch is on when nothing is saved", () => {
  expect(loadAllProjects()).toBe(true);
  expect(allProjectsSnapshot()).toBe(true);
});

test("unreadable storage reads on", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  expect(loadAllProjects()).toBe(true);
});

test("the switch reads off only after it is switched off", () => {
  saveAllProjects(false);
  expect(localStorage.getItem(ALL_PROJECTS_KEY)).toBe("0");
  expect(loadAllProjects()).toBe(false);
  saveAllProjects(true);
  expect(localStorage.getItem(ALL_PROJECTS_KEY)).toBe("1");
  expect(loadAllProjects()).toBe(true);
  // Anything but "0" is on, so a stray value never turns the feature off.
  localStorage.setItem(ALL_PROJECTS_KEY, "nope");
  expect(loadAllProjects()).toBe(true);
});

test("switching notifies subscribers", () => {
  const seen = vi.fn();
  const stop = subscribeAllProjects(seen);
  saveAllProjects(false);
  expect(seen).toHaveBeenCalledTimes(1);
  stop();
  saveAllProjects(true);
  expect(seen).toHaveBeenCalledTimes(1);
});

test("the switch is not saved during the tour", () => {
  const seen = vi.fn();
  const stop = subscribeAllProjects(seen);
  setTourRunning(true);
  saveAllProjects(false);
  expect(localStorage.getItem(ALL_PROJECTS_KEY)).toBeNull();
  expect(seen).not.toHaveBeenCalled();
  stop();
});

test("a switch that cannot be written does not throw", () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("full");
  });
  expect(() => saveAllProjects(false)).not.toThrow();
});

test("the effective project is the PBI's while on and home without a PBI", () => {
  expect(effectiveProject(true, { project: "Payroll" }, "HRM")).toBe("Payroll");
  expect(effectiveProject(true, null, "HRM")).toBe("HRM");
});

test("the effective project is home while off whatever the PBI says", () => {
  expect(effectiveProject(false, { project: "Payroll" }, "HRM")).toBe("HRM");
});

test("an empty PBI project falls back to home", () => {
  expect(effectiveProject(true, { project: "" }, "HRM")).toBe("HRM");
  expect(effectiveProject(true, {}, "HRM")).toBe("HRM");
});

test("the notice is offered once to an existing install", () => {
  expect(takeAllProjectsNotice(true)).toBe(true);
  expect(localStorage.getItem(ALL_PROJECTS_NOTICE_KEY)).not.toBeNull();
  expect(takeAllProjectsNotice(true)).toBe(false);
});

test("a fresh install never gets the notice", () => {
  expect(takeAllProjectsNotice(false)).toBe(false);
  // It was asked once, so a project picked later does not raise it.
  expect(takeAllProjectsNotice(true)).toBe(false);
});

test("the notice is not offered when the switch was already set", () => {
  saveAllProjects(false);
  expect(takeAllProjectsNotice(true)).toBe(false);
});

test("one project is not a span, two are, blanks do not count", () => {
  expect(spansProjects([])).toBe(false);
  expect(spansProjects([{ project: "HRM" }, { project: "HRM" }])).toBe(false);
  expect(spansProjects([{ project: "HRM" }, { project: "" }, { project: null }, {}])).toBe(false);
  expect(spansProjects([{ project: "HRM" }, { project: "Payroll" }])).toBe(true);
});

test("the key segment is the combined marker only while on", () => {
  expect(COMBINED).toBe("*");
  expect(scopeSegment("HRM", true)).toBe("*");
  expect(scopeSegment("HRM", false)).toBe("HRM");
});

test("seeding copies into an empty combined key and keeps the source", () => {
  localStorage.setItem("from", JSON.stringify([1, 2]));
  seedCombinedKey("to", "from");
  expect(localStorage.getItem("to")).toBe(JSON.stringify([1, 2]));
  expect(localStorage.getItem("from")).toBe(JSON.stringify([1, 2]));
});

test("seeding can reshape the copied value", () => {
  localStorage.setItem("from", JSON.stringify([{ id: 1 }]));
  seedCombinedKey("to", "from", (v) => (v as { id: number }[]).map((x) => ({ ...x, project: "HRM" })));
  expect(JSON.parse(localStorage.getItem("to") ?? "null")).toEqual([{ id: 1, project: "HRM" }]);
  expect(JSON.parse(localStorage.getItem("from") ?? "null")).toEqual([{ id: 1 }]);
});

test("seeding never overwrites a combined key that exists", () => {
  localStorage.setItem("from", "old");
  localStorage.setItem("to", "edited");
  seedCombinedKey("to", "from");
  expect(localStorage.getItem("to")).toBe("edited");
});

test("seeding twice does not copy again after the combined key is edited", () => {
  localStorage.setItem("from", JSON.stringify(["a"]));
  seedCombinedKey("to", "from");
  localStorage.setItem("to", JSON.stringify(["a", "b"]));
  seedCombinedKey("to", "from");
  expect(localStorage.getItem("to")).toBe(JSON.stringify(["a", "b"]));
});

test("seeding with nothing to copy writes nothing", () => {
  seedCombinedKey("to", "from");
  expect(localStorage.getItem("to")).toBeNull();
});

test("seeding bad saved data does not crash and writes nothing", () => {
  localStorage.setItem("from", "{not json");
  expect(() => seedCombinedKey("to", "from", (v) => v)).not.toThrow();
  expect(localStorage.getItem("to")).toBeNull();
  expect(localStorage.getItem("from")).toBe("{not json");
  // A reshaper that throws is bad data too.
  localStorage.setItem("from", "[1]");
  expect(() =>
    seedCombinedKey("to", "from", () => {
      throw new Error("bad");
    }),
  ).not.toThrow();
  expect(localStorage.getItem("to")).toBeNull();
});

test("seeding with unreadable storage does not crash", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  expect(() => seedCombinedKey("to", "from")).not.toThrow();
});

test("seeding is refused during the tour", () => {
  localStorage.setItem("from", "x");
  setTourRunning(true);
  seedCombinedKey("to", "from");
  expect(localStorage.getItem("to")).toBeNull();
});

test("seeding a reshaper that returns nothing writes nothing", () => {
  localStorage.setItem("from", "[1]");
  seedCombinedKey("to", "from", () => undefined);
  expect(localStorage.getItem("to")).toBeNull();
});
