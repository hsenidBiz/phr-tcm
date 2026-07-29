import { describe, expect, test } from "vitest";
import {
  canApply,
  EMPTY_RULE,
  MAX_TITLE,
  previewRename,
  rowsToApply,
  undoFromRows,
  type RenameRule,
} from "./powerRename";

const rule = (over: Partial<RenameRule> = {}): RenameRule => ({ ...EMPTY_RULE, ...over });
const cases = (...titles: string[]) => titles.map((title, i) => ({ id: 100 + i, title }));
const after = (p: ReturnType<typeof previewRename>) => p.rows.map((r) => r.after);

describe("find and replace", () => {
  test("plain text is literal, not a pattern", () => {
    // A find of "a.b" must not match "axb" - the commonest way a rename tool
    // surprises someone who has never heard of a regex.
    const p = previewRename(cases("a.b works", "axb works"), rule({ find: "a.b", replace: "Z" }));
    expect(after(p)).toEqual(["Z works", "axb works"]);
  });

  test("replaces every occurrence, or only the first", () => {
    const both = previewRename(cases("log in then log in again"), rule({ find: "log in", replace: "sign in" }));
    expect(after(both)).toEqual(["sign in then sign in again"]);

    const first = previewRename(
      cases("log in then log in again"),
      rule({ find: "log in", replace: "sign in", firstOnly: true }),
    );
    expect(after(first)).toEqual(["sign in then log in again"]);
  });

  test("case-insensitive by default, exact when asked", () => {
    const loose = previewRename(cases("Login works"), rule({ find: "login", replace: "Sign-in" }));
    expect(after(loose)).toEqual(["Sign-in works"]);

    const strict = previewRename(
      cases("Login works"),
      rule({ find: "login", replace: "Sign-in", matchCase: true }),
    );
    expect(after(strict)).toEqual(["Login works"]);
  });

  test("regex with capture groups", () => {
    const p = previewRename(
      cases("TC-001 - Login - valid", "TC-002 - Login - locked"),
      rule({ useRegex: true, find: "^TC-(\\d+) - (.+)$", replace: "$2 [$1]" }),
    );
    expect(after(p)).toEqual(["Login - valid [001]", "Login - locked [002]"]);
  });

  test("a broken regex reports the parser's own message and changes nothing", () => {
    const p = previewRename(cases("Login works"), rule({ useRegex: true, find: "(unclosed", replace: "x" }));
    expect(p.error).toBeTruthy();
    expect(after(p)).toEqual(["Login works"]);
    expect(canApply(p)).toBe(false);
  });
});

describe("casing", () => {
  test("upper and lower", () => {
    expect(after(previewRename(cases("Login works"), rule({ casing: "upper" })))).toEqual([
      "LOGIN WORKS",
    ]);
    expect(after(previewRename(cases("Login Works"), rule({ casing: "lower" })))).toEqual([
      "login works",
    ]);
  });

  /** Test case titles are full of acronyms, and the textbook implementation
   *  turns "API" into "Api". A word that already carries a capital is left
   *  exactly as it is. */
  test("title case capitalises words without destroying acronyms", () => {
    const p = previewRename(
      cases("the API returns a PBI id", "login page loads"),
      rule({ casing: "title" }),
    );
    expect(after(p)).toEqual(["The API Returns A PBI Id", "Login Page Loads"]);
  });
});

describe("prefix, suffix and numbering", () => {
  test("affixes wrap the result, and casing runs before them", () => {
    // An UPPER rule must not shout a prefix the user typed in mixed case.
    const p = previewRename(cases("login works"), rule({ casing: "upper", prefix: "Smoke: ", suffix: " (v2)" }));
    expect(after(p)).toEqual(["Smoke: LOGIN WORKS (v2)"]);
  });

  test("${n} counts in display order, from the start value, padded", () => {
    const p = previewRename(
      cases("Login", "Logout", "Reset"),
      rule({ prefix: "TC-${n} - ", numberFrom: 7, numberPad: 3 }),
    );
    expect(after(p)).toEqual(["TC-007 - Login", "TC-008 - Logout", "TC-009 - Reset"]);
  });

  test("${n} works in the replacement too", () => {
    const p = previewRename(
      cases("Case A", "Case B"),
      rule({ find: "Case", replace: "Step ${n}", numberFrom: 1, numberPad: 2 }),
    );
    expect(after(p)).toEqual(["Step 01 A", "Step 02 B"]);
  });
});

describe("what the preview refuses", () => {
  test("an empty result is blocked, not written", () => {
    const p = previewRename(cases("Login"), rule({ find: "Login", replace: "" }));
    expect(p.rows[0].status).toEqual({ kind: "blocked", reason: "A title cannot be empty." });
    expect(p.blocked).toBe(1);
    expect(canApply(p)).toBe(false);
    expect(rowsToApply(p)).toEqual([]);
  });

  test("over the Azure DevOps limit is blocked, with the real count", () => {
    const p = previewRename(cases("Login"), rule({ suffix: "x".repeat(MAX_TITLE) }));
    expect(p.rows[0].status.kind).toBe("blocked");
    if (p.rows[0].status.kind === "blocked") {
      expect(p.rows[0].status.reason).toContain(String(MAX_TITLE));
    }
    expect(canApply(p)).toBe(false);
  });

  /** A collision is legal - this app treats a matching title as a duplicate
   *  and never as an update - so it warns and still applies. */
  test("a collision warns but does not block", () => {
    const withinSelection = previewRename(
      cases("Login valid", "Login locked"),
      rule({ useRegex: true, find: " (valid|locked)$", replace: "" }),
    );
    expect(withinSelection.warned).toBe(2);
    expect(withinSelection.blocked).toBe(0);
    expect(canApply(withinSelection)).toBe(true);

    const againstTheRest = previewRename(
      cases("Login v2"),
      rule({ find: " v2", replace: "" }),
      ["Login"],
    );
    expect(againstTheRest.rows[0].status.kind).toBe("warned");
    expect(canApply(againstTheRest)).toBe(true);
  });

  test("an unchanged row is neither applied nor counted", () => {
    const p = previewRename(cases("Login", "Logout"), rule({ find: "Login", replace: "Sign in" }));
    expect(p.renamed).toBe(1);
    expect(p.rows[1].status).toEqual({ kind: "unchanged" });
    expect(rowsToApply(p).map((r) => r.before)).toEqual(["Login"]);
  });

  test("an empty rule changes nothing and cannot be applied", () => {
    const p = previewRename(cases("Login", "Logout"), rule());
    expect(after(p)).toEqual(["Login", "Logout"]);
    expect(p.renamed).toBe(0);
    expect(canApply(p)).toBe(false);
  });
});

/** Undo is the same write in the other direction - this app has no DELETE
 *  and does not roll back revisions, so there is nothing else it could be. */
test("undo carries the titles back", () => {
  const p = previewRename(cases("Login", "Logout"), rule({ prefix: "Smoke: " }));
  const undo = undoFromRows(rowsToApply(p), 1234);
  expect(undo.entries).toEqual([
    { id: 100, from: "Smoke: Login", to: "Login" },
    { id: 101, from: "Smoke: Logout", to: "Logout" },
  ]);
  expect(undo.at).toBe(1234);
});

/** The whole reason this module exists: the strings shown are the strings
 *  sent. Nothing downstream recomputes them from the rule. */
test("what is applied is exactly what the preview showed", () => {
  const p = previewRename(
    cases("TC-1 login", "TC-2 logout", "no code here"),
    rule({ useRegex: true, find: "^TC-(\\d+) ", replace: "", prefix: "Auth ${n}: ", casing: "title" }),
  );
  const applied = rowsToApply(p);
  for (const row of applied) {
    const shown = p.rows.find((r) => r.id === row.id);
    expect(row.after).toBe(shown?.after);
  }
  // The prefix and casing apply to every selected case, including the one
  // the find never matched - selecting a case is what opts it in.
  expect(applied.map((r) => r.after)).toEqual([
    "Auth 1: Login",
    "Auth 2: Logout",
    "Auth 3: No Code Here",
  ]);
});
