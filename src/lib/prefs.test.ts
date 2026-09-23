// A section missing from prefs.ts's own whitelist reads back as "manual"
// forever - that hid a real bug (Auto Run losing a saved tab on every
// restart; see lib/extras.ts's shouldLeaveAutoRun). Enumerating every real
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
