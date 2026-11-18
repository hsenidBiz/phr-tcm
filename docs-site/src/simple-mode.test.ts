import { describe, expect, test } from "vitest";
import { screens } from "./content";

// App Simple mode hides six screens. The guide still documents them (they
// come back when it is turned off), and says so on each one.

const HIDDEN = "Hidden while Simple mode is on (Settings).";
const screen = (id: string) => {
  const s = screens.find((x) => x.id === id);
  if (!s) throw new Error(`no screen ${id}`);
  return s;
};

describe("Simple mode in the guide", () => {
  test.each(["import-file", "update-test-cases", "search-suites", "suite-management", "api-templates", "playwright"])(
    "%s says it is hidden while Simple mode is on",
    (id) => {
      expect(screen(id).summary).toContain(HIDDEN);
    },
  );

  test("Settings explains Simple mode and says the Modules page is hidden by it", () => {
    const s = screen("settings");
    const group = s.groups?.find((g) => g.id === "simple-mode");
    expect(group?.title).toBe("Simple mode");
    const control = s.controls.find((c) => c.group === "simple-mode");
    expect(control?.locate).toEqual({ role: "switch", name: "Simple mode" });
    expect(JSON.stringify(s.controls.find((c) => c.group === "simple-mode"))).toContain("Playwright");
    expect(s.groups?.find((g) => g.id === "modules")?.summary).toContain(HIDDEN);
  });

  test("a shot shows the Simple mode switch on", () => {
    const shot = screen("settings").shots.find((x) => x.id === "settings-simple-mode");
    expect(shot).toBeDefined();
    expect(shot?.route).toContainEqual({ click: { role: "switch", name: "Simple mode" } });
  });

  test("Getting started names the mode and says the shortcut numbers follow the sidebar", () => {
    const text = JSON.stringify(screen("getting-started"));
    expect(text).toContain("Simple mode");
    expect(text).toContain("follow the sidebar");
    expect(text).toContain("[[Ctrl+4]] is AI Bridge");
  });
});
