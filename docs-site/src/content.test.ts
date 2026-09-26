import { describe, expect, test } from "vitest";
import realPositions from "../shots/positions.json";
import { screens } from "./content";
import type { Positions, Screen } from "./types";
import { locateProblem, positionsProblems, validateContent } from "./validate";

// The same function `npm run docs:shots -- --validate` runs.

const good = (): Screen[] => [
  {
    id: "alpha",
    title: "Alpha",
    group: "Test cases",
    summary: "The alpha screen.",
    shots: [
      { id: "alpha-main", route: [{ nav: "Alpha" }], alt: "Alpha" },
      { id: "alpha-open", route: [{ nav: "Alpha" }, { click: { role: "button", name: "Open" } }], alt: "Alpha, open" },
    ],
    controls: [
      { id: "open", shot: "alpha-main", locate: { role: "button", name: "Open" }, name: "Open", does: "Opens it." },
      { id: "count", shot: "alpha-open", locate: { role: "button", nameRe: "^Run \\d+$" }, name: "Run", does: "Runs." },
    ],
  },
  {
    id: "beta",
    title: "Beta",
    group: "Settings",
    summary: "The beta screen.",
    shots: [{ id: "beta-main", route: [{ nav: "Beta" }, { runnerWindow: true }, { press: "Escape" }], alt: "Beta" }],
    controls: [
      { id: "open", shot: "beta-main", locate: { label: "Filter" }, name: "Filter", does: "Filters." },
      { id: "tab", shot: "beta-main", locate: { testId: "nav-beta" }, name: "Tab", does: "A tab." },
      { id: "title", shot: "beta-main", locate: { text: "Beta" }, name: "Title", does: "The title." },
    ],
  },
];

describe("validateContent", () => {
  test("well-formed content has no problems (a control id may repeat across screens)", () => {
    expect(validateContent({ screens: good() })).toEqual([]);
  });

  test("the empty registry is valid", () => {
    expect(validateContent({ screens: [] })).toEqual([]);
  });

  test("the real registry is valid", () => {
    expect(validateContent({ screens })).toEqual([]);
  });

  test("names a control whose shot does not exist on its screen", () => {
    const s = good();
    s[0].controls[0].shot = "alpha-missing";
    s[0].controls[1].shot = "beta-main"; // exists, but on another screen
    expect(validateContent({ screens: s })).toEqual([
      'screen "alpha", control "open": shot "alpha-missing" is not one of this screen\'s shots',
      'screen "alpha", control "count": shot "beta-main" is not one of this screen\'s shots',
    ]);
  });

  test("names duplicate screen, shot and control ids", () => {
    const s = good();
    s[1].id = "alpha";
    s[1].shots[0].id = "alpha-main";
    s[1].controls[1].id = "open";
    const problems = validateContent({ screens: s });
    expect(problems).toContain('screen id "alpha" is used more than once');
    expect(problems).toContain('shot id "alpha-main" is used more than once');
    expect(problems).toContain('screen "alpha": control id "open" is used more than once');
  });

  test("names a shot without a route and a malformed step", () => {
    const s = good();
    s[0].shots[0].route = [];
    s[0].shots[1].route = [{ nav: "Alpha" }, { hover: "x" } as never, { click: { role: "button" } } as never];
    expect(validateContent({ screens: s })).toEqual([
      'screen "alpha", shot "alpha-main": has no route',
      'screen "alpha", shot "alpha-open": route step 2 is an unknown step "hover"',
      'screen "alpha", shot "alpha-open": route step 3 locate has an unknown shape {role}',
    ]);
  });

  test("ids that become file names and anchors must be kebab-case", () => {
    const s = good();
    s[0].shots[0].id = "Alpha Main";
    s[0].controls[0].shot = "Alpha Main";
    s[0].controls[0].id = "open_it";
    expect(validateContent({ screens: s })).toEqual([
      'screen "alpha", shot "Alpha Main": id is not lower-case kebab-case',
      'screen "alpha", control "open_it": id is not lower-case kebab-case',
    ]);
  });
});

describe("shot sizes and positions", () => {
  const withRunner = (): Screen[] => {
    const s = good();
    s[1].shots.push({ id: "beta-runner", route: [{ runnerWindow: true }], alt: "Runner", size: { w: 460, h: 720 } });
    s[1].controls.push({ id: "close", shot: "beta-runner", locate: { role: "button", name: "Close runner" }, name: "Close", does: "Closes." });
    return s;
  };

  test("a shot may give its own size, in whole pixels", () => {
    expect(validateContent({ screens: withRunner() })).toEqual([]);
    const s = withRunner();
    s[1].shots[1].size = { w: 460.5, h: 0 };
    expect(validateContent({ screens: s })).toEqual(['screen "beta", shot "beta-runner": size must be whole pixels above zero']);
  });

  test("positions that match the content have no problems (a control without a box is fine)", () => {
    const positions: Positions = {
      "alpha-main": { size: { w: 1440, h: 900 }, controls: { open: { x: 1, y: 2, w: 3, h: 4 } } },
      "beta-runner": { size: { w: 460, h: 720 }, controls: { close: { x: 1, y: 2, w: 3, h: 4 } } },
    };
    expect(positionsProblems(withRunner(), positions)).toEqual([]);
  });

  test("names positions for an unknown shot or control, and a size that no longer matches", () => {
    const positions: Positions = {
      gone: { size: { w: 1440, h: 900 }, controls: {} },
      "alpha-main": { size: { w: 1440, h: 900 }, controls: { count: { x: 1, y: 2, w: 3, h: 4 } } },
      "beta-runner": { size: { w: 1440, h: 900 }, controls: {} },
    };
    expect(positionsProblems(withRunner(), positions)).toEqual([
      'positions: shot "alpha-main" has a box for "count", which is not documented on it',
      'positions: shot "beta-runner" was captured at 1440x900, the content says 460x720',
      'positions: shot "gone" is not in the content',
    ]);
  });

  test("the committed positions.json matches the real registry", () => {
    expect(positionsProblems(screens, realPositions as Positions)).toEqual([]);
  });
});

describe("locateProblem", () => {
  test("accepts every documented shape", () => {
    for (const l of [
      { role: "button", name: "Open" },
      { role: "button", nameRe: "^Run \\d+$" },
      { label: "Filter" },
      { text: "Steps" },
      { testId: "nav-run" },
    ]) {
      expect(locateProblem(l)).toBeNull();
    }
  });

  test("rejects a missing, empty, mixed or invalid locate", () => {
    expect(locateProblem(undefined)).toBe("has no locate");
    expect(locateProblem({ role: "button", name: " " })).toBe("locate needs a non-empty role and name");
    expect(locateProblem({ label: "" })).toBe("locate label is empty");
    expect(locateProblem({ label: "A", text: "B" })).toBe("locate has an unknown shape {label,text}");
    expect(locateProblem({ role: "button", nameRe: "(" })).toBe("locate nameRe is not a valid pattern: (");
  });
});
