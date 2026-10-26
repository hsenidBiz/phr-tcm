// A section missing from prefs.ts's own whitelist reads back as "manual"
// forever - that hid a real bug (Auto Run losing a saved tab on every
// restart; see lib/extras.ts's shouldLeaveHidden). Enumerating every real
// Section id, rather than a hand-picked few, is what would have caught it.
import { afterEach, expect, test } from "vitest";
import { CASE_ITEMS } from "../components/Sidebar";
import { loadPrefs, PREFS_KEY, savePrefs, type Prefs } from "./prefs";

afterEach(() => {
  localStorage.clear();
});

const base: Prefs = { org: "", project: "", section: "manual", pbi: null, workMode: false };

const sections = [...CASE_ITEMS.map((i) => i.id), "settings"] as const;

test.each(sections)("a saved %s section survives a reload", (section) => {
  savePrefs({ ...base, section });
  expect(loadPrefs().section).toBe(section);
});

test("an unrecognised section in storage falls back to Manual Entry", () => {
  localStorage.setItem(PREFS_KEY, JSON.stringify({ ...base, section: "nonsense" }));
  expect(loadPrefs().section).toBe("manual");
});

const storedPbi = { id: 7, title: "Seven", work_item_type: "Product Backlog Item" };

test("a saved PBI without a project takes the saved project", () => {
  // What a person saved before PBIs carried a project.
  localStorage.setItem(PREFS_KEY, JSON.stringify({ ...base, org: "acme", project: "HRM", pbi: storedPbi }));
  const loaded = loadPrefs();
  expect(loaded.pbi).toEqual({ ...storedPbi, project: "HRM" });
  // The saved project stays the home project, untouched.
  expect(loaded.org).toBe("acme");
  expect(loaded.project).toBe("HRM");
});

test("a saved PBI keeps its own project", () => {
  localStorage.setItem(
    PREFS_KEY,
    JSON.stringify({ ...base, project: "HRM", pbi: { ...storedPbi, project: "Payroll" } }),
  );
  const loaded = loadPrefs();
  expect(loaded.pbi?.project).toBe("Payroll");
  expect(loaded.project).toBe("HRM");
});

test("a saved PBI with an empty or odd project takes the saved project, and none at all gives empty", () => {
  localStorage.setItem(PREFS_KEY, JSON.stringify({ ...base, project: "HRM", pbi: { ...storedPbi, project: "" } }));
  expect(loadPrefs().pbi?.project).toBe("HRM");
  localStorage.setItem(PREFS_KEY, JSON.stringify({ ...base, project: "HRM", pbi: { ...storedPbi, project: 5 } }));
  expect(loadPrefs().pbi?.project).toBe("HRM");
  localStorage.setItem(PREFS_KEY, JSON.stringify({ section: "manual", pbi: storedPbi }));
  expect(loadPrefs().pbi?.project).toBe("");
});

test("corrupt saved prefs load as defaults", () => {
  localStorage.setItem(PREFS_KEY, "{not json");
  expect(loadPrefs()).toEqual(base);
});
