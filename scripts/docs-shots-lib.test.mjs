import { describe, expect, test } from "vitest";
import {
  boxInShot,
  formatAge,
  formatMissing,
  KEYS,
  locatorFor,
  mergePositions,
  navLocator,
  parseArgs,
  passStorage,
  pickShots,
  planIsEmpty,
  preflight,
  restoreInOrder,
  restorePlan,
  roundBox,
  runStep,
  serializePositions,
  shotSize,
  STALE_BACKUP_MS,
  viewportOf,
  visibleOnly,
} from "./docs-shots-lib.mjs";

/** A stand-in for a Playwright Page/Locator that records every call made
 *  on it and on whatever it returns. */
function recorder() {
  const calls = [];
  // `path` reads like the call chain: "getByRole().click".
  const make = (path) =>
    new Proxy(function () {}, {
      get: (_t, prop) => (prop === "then" ? undefined : make(path ? `${path}.${String(prop)}` : String(prop))),
      apply: (_t, _this, args) => {
        calls.push([path, ...args]);
        return make(`${path}()`);
      },
    });
  return { root: make(""), calls };
}

describe("locatorFor", () => {
  test("role + name is an exact getByRole", () => {
    const { root, calls } = recorder();
    locatorFor(root, { role: "button", name: "Upload" });
    expect(calls).toEqual([["getByRole", "button", { name: "Upload", exact: true }]]);
  });

  test("role + nameRe passes a RegExp built from the source", () => {
    const { root, calls } = recorder();
    locatorFor(root, { role: "button", nameRe: "^Run \\d+ in runner$" });
    const [[fn, role, opts]] = calls;
    expect([fn, role]).toEqual(["getByRole", "button"]);
    expect(opts.name).toBeInstanceOf(RegExp);
    expect(opts.name.source).toBe("^Run \\d+ in runner$");
    expect(opts.name.test("Run 3 in runner")).toBe(true);
  });

  test("label and text are exact; testId matches data-testid or data-tour", () => {
    const { root, calls } = recorder();
    locatorFor(root, { label: "Filter" });
    locatorFor(root, { text: "Steps" });
    locatorFor(root, { testId: 'nav-"run"' });
    expect(calls).toEqual([
      ["getByLabel", "Filter", { exact: true }],
      ["getByText", "Steps", { exact: true }],
      ["locator", '[data-testid="nav-\\"run\\""], [data-tour="nav-\\"run\\""]'],
    ]);
  });

  test("an unknown shape throws", () => {
    expect(() => locatorFor(recorder().root, { css: ".x" })).toThrow(/Unknown locate/);
  });
});

describe("navLocator", () => {
  test("finds the visible sidebar button by its exact label - no badge form is accepted", () => {
    const { root, calls } = recorder();
    navLocator(root, "Run Tests");
    expect(calls).toEqual([
      ["getByRole", "navigation"],
      ["getByRole().getByRole", "button", { name: "Run Tests", exact: true }],
      ["getByRole().getByRole().filter", { visible: true }],
      ["getByRole().getByRole().filter().first"],
    ]);
  });
});

test("visibleOnly filters a locator to what a person can see", () => {
  const { root, calls } = recorder();
  visibleOnly(root);
  expect(calls).toEqual([["filter", { visible: true }]]);
});

describe("runStep", () => {
  test("maps each step to its Playwright call and keeps the target", async () => {
    const { root, calls } = recorder();
    const ctx = { page: root, openRunner: async () => "runner" };
    expect(await runStep(ctx, { click: { role: "button", name: "Open" } })).toBe(root);
    await runStep(ctx, { press: "Control+K" });
    await runStep(ctx, { waitFor: { text: "Done" } });
    await runStep(ctx, { scrollTo: { label: "Notes" } });
    expect(calls).toEqual([
      ["getByRole", "button", { name: "Open", exact: true }],
      ["getByRole().filter", { visible: true }],
      ["getByRole().filter().click"],
      ["keyboard.press", "Control+K"],
      ["getByText", "Done", { exact: true }],
      ["getByText().filter", { visible: true }],
      ["getByText().filter().waitFor", { state: "visible" }],
      ["getByLabel", "Notes", { exact: true }],
      ["getByLabel().filter", { visible: true }],
      ["getByLabel().filter().scrollIntoViewIfNeeded"],
    ]);
  });

  test("runnerWindow switches the target to the runner", async () => {
    const { root } = recorder();
    expect(await runStep({ page: root, openRunner: async () => "runner" }, { runnerWindow: true })).toBe("runner");
  });

  test("nav clicks the sidebar item", async () => {
    const { root, calls } = recorder();
    await runStep({ page: root, openRunner: async () => null }, { nav: "Settings" });
    expect(calls.at(-1)).toEqual(["getByRole().getByRole().filter().first().click"]);
  });
});

describe("storage plans", () => {
  test("a pass turns capture mode on, sets the theme and pins the look", () => {
    expect(passStorage("slate")).toEqual({
      set: { [KEYS.capture]: "on", [KEYS.themeId]: "slate" },
      remove: [KEYS.themeLegacy, KEYS.accent, KEYS.sidebar],
    });
  });

  test("restore sets back changed and deleted keys and removes new ones, nothing else", () => {
    const snapshot = { a: "1", b: "2", c: "3", untouched: "x" };
    const current = { a: "1", b: "changed", untouched: "x", added: "y", "added-2": "z" };
    expect(restorePlan(snapshot, current)).toEqual({ set: { b: "2", c: "3" }, remove: ["added", "added-2"] });
  });

  test("restoring an unchanged store is a no-op", () => {
    const s = { a: "1", "tcm-v2-prefs": "{}" };
    expect(planIsEmpty(restorePlan(s, { ...s }))).toBe(true);
    expect(planIsEmpty(restorePlan(s, { a: "1" }))).toBe(false);
  });

  test("an empty string value is restored, not treated as missing", () => {
    expect(restorePlan({ k: "" }, {})).toEqual({ set: { k: "" }, remove: [] });
  });

  test("applying the plan to the current store gives back the snapshot exactly", () => {
    const snapshot = { [KEYS.demo]: "off", [KEYS.themeId]: "ocean", [KEYS.accent]: "rose", "tcm-v2-cache:orgs": "[1]" };
    const current = { ...snapshot };
    const pass = passStorage("light");
    Object.assign(current, pass.set, { [KEYS.demo]: "on", "tcm-v2-recent-pbis:DemoOrg/Demo Project": "[]" });
    for (const k of pass.remove) delete current[k];
    delete current["tcm-v2-cache:orgs"]; // the app wiped a cache while signed in as someone else
    const plan = restorePlan(snapshot, current);
    Object.assign(current, plan.set);
    for (const k of plan.remove) delete current[k];
    expect(current).toEqual(snapshot);
  });
});

describe("positions", () => {
  test("boxes are rounded to whole pixels", () => {
    expect(roundBox({ x: 10.4, y: 20.5, width: 99.6, height: 31.49 })).toEqual({ x: 10, y: 21, w: 100, h: 31 });
  });

  test("a box counts as in the shot when its centre is inside the shot's own size", () => {
    expect(boxInShot({ x: 10, y: 10, width: 20, height: 20 })).toBe(true);
    expect(boxInShot({ x: 1430, y: 10, width: 40, height: 20 })).toBe(false);
    expect(boxInShot({ x: 10, y: 895, width: 20, height: 4 })).toBe(true);
    expect(boxInShot({ x: 10, y: -30, width: 20, height: 20 })).toBe(false);
    expect(boxInShot({ x: 10, y: 10, width: 0, height: 20 })).toBe(false);
    const runner = { w: 460, h: 720 };
    expect(boxInShot({ x: 400, y: 690, width: 20, height: 20 }, runner)).toBe(true);
    expect(boxInShot({ x: 500, y: 10, width: 20, height: 20 }, runner)).toBe(false);
  });

  test("a shot is captured at the main window's size unless it sets its own", () => {
    expect(shotSize({ id: "a" })).toEqual({ w: 1440, h: 900 });
    expect(shotSize({ id: "b", size: { w: 460, h: 720 } })).toEqual({ w: 460, h: 720 });
    expect(viewportOf({ w: 460, h: 720 })).toEqual({ width: 460, height: 720 });
  });

  test("written with sorted shots, each with its size, then sorted controls in x/y/w/h order", () => {
    const text = serializePositions({
      "run-main": { controls: { upload: { h: 4, w: 3, y: 2, x: 1 }, alpha: { x: 5, y: 6, w: 7, h: 8 } }, size: { h: 900, w: 1440 } },
      "import-main": { size: { w: 460, h: 720 }, controls: { zed: { x: 0, y: 0, w: 1, h: 1 } } },
    });
    expect(text).toBe(
      `${JSON.stringify(
        {
          "import-main": { size: { w: 460, h: 720 }, controls: { zed: { x: 0, y: 0, w: 1, h: 1 } } },
          "run-main": { size: { w: 1440, h: 900 }, controls: { alpha: { x: 5, y: 6, w: 7, h: 8 }, upload: { x: 1, y: 2, w: 3, h: 4 } } },
        },
        null,
        2,
      )}\n`,
    );
    expect(Object.keys(JSON.parse(text))).toEqual(["import-main", "run-main"]);
  });

  test("the same positions always serialise the same", () => {
    const size = { w: 1440, h: 900 };
    const one = { x: 1, y: 1, w: 1, h: 1 };
    const two = { x: 2, y: 2, w: 2, h: 2 };
    const a = serializePositions({ b: { size, controls: { y: one, x: two } }, a: { size, controls: {} } });
    const b = serializePositions({ a: { size, controls: {} }, b: { size, controls: { x: two, y: one } } });
    expect(a).toBe(b);
  });
});

describe("parseArgs", () => {
  test("modes", () => {
    expect(parseArgs([])).toEqual({ mode: "capture" });
    expect(parseArgs(["--validate"])).toEqual({ mode: "validate" });
    expect(parseArgs(["--dry-run"])).toEqual({ mode: "dry-run" });
    expect(parseArgs(["--restore"])).toEqual({ mode: "restore" });
    expect(parseArgs(["--help"])).toEqual({ mode: "help" });
  });

  test("an unknown option or two modes is a usage error", () => {
    expect(parseArgs(["--fast"])).toEqual({ mode: "help", error: "Unknown option: --fast" });
    expect(parseArgs(["--validate", "--dry-run"]).error).toMatch(/Pick one/);
  });

  test("--only takes shot ids, spaced or with =, with a capture or a dry run", () => {
    expect(parseArgs(["--only", "a,b"])).toEqual({ mode: "capture", only: ["a", "b"] });
    expect(parseArgs(["--only=a", "--dry-run"])).toEqual({ mode: "dry-run", only: ["a"] });
    expect(parseArgs(["--only", "a", "--only", "b,a"])).toEqual({ mode: "capture", only: ["a", "b"] });
    expect(parseArgs(["--only"]).error).toMatch(/needs shot ids/);
    expect(parseArgs(["--only", "--dry-run"]).error).toMatch(/needs shot ids/);
    expect(parseArgs(["--only", "a", "--restore"]).error).toMatch(/goes with a capture or --dry-run/);
  });

  test("--force goes with --restore only", () => {
    expect(parseArgs(["--restore", "--force"])).toEqual({ mode: "restore", force: true });
    expect(parseArgs(["--force"]).error).toMatch(/only goes with --restore/);
  });
});

describe("preflight (decided before the content is loaded)", () => {
  const NOW = 1_800_000_000_000;
  const PATH = "C:/tmp/backup.json";
  const fresh = { at: NOW - 5 * 60_000 };
  const old = { at: NOW - STALE_BACKUP_MS - 60_000 };

  test("--restore restores a recent snapshot, or says there is nothing to do", () => {
    expect(preflight({ mode: "restore" }, fresh, PATH, NOW)).toEqual({ action: "restore" });
    expect(preflight({ mode: "restore" }, null, PATH, NOW)).toEqual({
      action: "exit",
      code: 0,
      message: "Nothing to restore: no interrupted run left a snapshot behind.",
    });
  });

  test("a snapshot more than a day old, or of unknown age, needs --force, and says how old it is", () => {
    const r = preflight({ mode: "restore" }, old, PATH, NOW);
    expect(r).toMatchObject({ action: "exit", code: 1 });
    expect(r.message).toContain("taken 24 hours ago");
    expect(r.message).toContain("--restore --force");
    expect(r.message).toContain(PATH);
    expect(preflight({ mode: "restore" }, {}, PATH, NOW).message).toContain("at an unknown time");
    expect(preflight({ mode: "restore", force: true }, old, PATH, NOW)).toEqual({ action: "restore" });
  });

  test("a capture or dry run refuses while a snapshot is left over, naming its age", () => {
    for (const mode of ["capture", "dry-run"]) {
      const r = preflight({ mode }, fresh, PATH, NOW);
      expect(r).toMatchObject({ action: "exit", code: 1 });
      expect(r.message).toContain("An earlier capture (5 minutes ago)");
      expect(r.message).toContain("--restore");
    }
    expect(preflight({ mode: "capture" }, null, PATH, NOW)).toEqual({ action: "content" });
  });

  test("--validate ignores any snapshot", () => {
    expect(preflight({ mode: "validate" }, fresh, PATH, NOW)).toEqual({ action: "content" });
  });

  test("ages read naturally", () => {
    expect(formatAge(0)).toBe("under a minute");
    expect(formatAge(59_000)).toBe("under a minute");
    expect(formatAge(60_000)).toBe("1 minute");
    expect(formatAge(59 * 60_000)).toBe("59 minutes");
    expect(formatAge(3 * 3_600_000)).toBe("3 hours");
    expect(formatAge(47 * 3_600_000)).toBe("47 hours");
    expect(formatAge(72 * 3_600_000)).toBe("3 days");
  });
});

test("formatMissing names shot, control and locate", () => {
  expect(
    formatMissing([{ shot: "run-main", control: "upload", locate: { role: "button", name: "Upload" }, reason: "not found" }]),
  ).toBe('  run-main / upload / {"role":"button","name":"Upload"}: not found');
});

describe("--only", () => {
  const shots = [{ shot: { id: "a" } }, { shot: { id: "b" } }, { shot: { id: "c" } }];

  test("picks just the named shots, and names any the content does not have", () => {
    expect(pickShots(shots, undefined)).toEqual({ shots, unknown: [] });
    expect(pickShots(shots, ["c", "a"])).toEqual({ shots: [shots[0], shots[2]], unknown: [] });
    expect(pickShots(shots, ["a", "zz"]).unknown).toEqual(["zz"]);
  });

  test("the shots taken replace their own positions and leave every other shot's alone", () => {
    const box = (x) => ({ x, y: 0, w: 1, h: 1 });
    const existing = {
      a: { size: { w: 1440, h: 900 }, controls: { one: box(1) } },
      b: { size: { w: 1440, h: 900 }, controls: { two: box(2) } },
    };
    const fresh = { b: { size: { w: 1440, h: 900 }, controls: { two: box(9), three: box(3) } } };
    expect(mergePositions(existing, fresh)).toEqual({ a: existing.a, b: fresh.b });
  });
});

describe("restoreInOrder", () => {
  test("closes the runner first, then puts the settings back", async () => {
    const order = [];
    const problem = await restoreInOrder({
      closeRunners: async () => void order.push("close"),
      putBack: async () => void order.push("put back"),
    });
    expect(order).toEqual(["close", "put back"]);
    expect(problem).toBeNull();
  });

  test("a runner that will not close still gets the settings put back, and the problem is handed back", async () => {
    let putBack = false;
    const problem = await restoreInOrder({
      closeRunners: async () => {
        throw new Error("the runner window would not close");
      },
      putBack: async () => {
        putBack = true;
      },
    });
    expect(putBack).toBe(true);
    expect(problem?.message).toBe("the runner window would not close");
  });

  test("a failed put back is not swallowed", async () => {
    await expect(
      restoreInOrder({
        closeRunners: async () => {},
        putBack: async () => {
          throw new Error("could not put back: k");
        },
      }),
    ).rejects.toThrow("could not put back: k");
  });
});
