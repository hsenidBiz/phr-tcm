import { describe, expect, test } from "vitest";
import { caseSearchFields, matchesQuery, searchAlternatives, searchPlaceholder } from "./search";

describe("matchesQuery", () => {
  const fields = [4321, "Login - Locked Account", "smoke; regression", "Active", null, undefined];

  test("an empty or blank query matches everything", () => {
    expect(matchesQuery(fields, "")).toBe(true);
    expect(matchesQuery(fields, "   ")).toBe(true);
    expect(matchesQuery([], "")).toBe(true);
  });

  test("is case-insensitive", () => {
    expect(matchesQuery(fields, "LOCKED")).toBe(true);
    expect(matchesQuery(fields, "Smoke")).toBe(true);
  });

  test("a word may match any one field", () => {
    expect(matchesQuery(fields, "regression")).toBe(true);
    expect(matchesQuery(fields, "active")).toBe(true);
    expect(matchesQuery(fields, "nothing")).toBe(false);
  });

  test("every word must match (AND), each in any field", () => {
    expect(matchesQuery(fields, "locked smoke")).toBe(true);
    expect(matchesQuery(fields, "locked active 43")).toBe(true);
    expect(matchesQuery(fields, "locked expired")).toBe(false);
  });

  test("a quoted phrase is one word", () => {
    expect(matchesQuery(fields, '"locked account"')).toBe(true);
    expect(matchesQuery(fields, '"account locked"')).toBe(false);
    expect(matchesQuery(fields, '"locked account" smoke')).toBe(true);
    expect(matchesQuery(fields, '"locked account" nothing')).toBe(false);
  });

  test("an unclosed quote still searches for what follows it", () => {
    expect(matchesQuery(fields, '"locked acc')).toBe(true);
  });

  test("a number field is an id and also matches with a leading #", () => {
    expect(matchesQuery(fields, "4321")).toBe(true);
    expect(matchesQuery(fields, "#4321")).toBe(true);
    expect(matchesQuery(fields, "#43")).toBe(true);
    expect(matchesQuery(fields, "#99")).toBe(false);
    expect(matchesQuery(fields, "#4321 smoke")).toBe(true);
  });

  test("a # does not match text fields that merely contain digits", () => {
    expect(matchesQuery(["case 12"], "#12")).toBe(false);
  });

  test("null and undefined fields are ignored", () => {
    expect(matchesQuery([null, undefined], "x")).toBe(false);
    expect(matchesQuery([null, "x"], "x")).toBe(true);
  });
});

describe("caseSearchFields", () => {
  const c = {
    id: 5,
    title: "Pay run",
    tags: "smoke",
    module_value: "Payroll",
    automation_status: "Planned",
    priority: 2,
  };

  test("finds a case by any of its listed fields, including priority as P2", () => {
    for (const q of ["#5", "pay run", "smoke", "payroll", "planned", "p2"]) {
      expect(matchesQuery(caseSearchFields(c), q)).toBe(true);
    }
    expect(matchesQuery(caseSearchFields(c), "p3")).toBe(false);
    expect(matchesQuery(caseSearchFields({ ...c, priority: null }), "p2")).toBe(false);
  });
});

describe("searchPlaceholder", () => {
  test("lists the names", () => {
    expect(searchPlaceholder(["name", "id"])).toBe("Search by name, id");
    expect(searchPlaceholder(["name", "id", "tag", "state"])).toBe("Search by name, id, tag, state");
  });

  test("shows only the first four and an ellipsis when there are more", () => {
    expect(searchPlaceholder(["name", "id", "tag", "state", "priority"])).toBe(
      "Search by name, id, tag, state…",
    );
  });
});

describe("commas", () => {
  const login = [1234, "Login error on submit", "smoke"];
  const timeout = [5678, "Report timeout", "regression"];
  const other = [9999, "Something else", ""];

  test("a comma searches for several: a row matching ANY alternative is found", () => {
    const hit = (r: (string | number)[], q: string) => matchesQuery(r, q);
    expect(hit(login, "1234, 5678")).toBe(true);
    expect(hit(timeout, "1234, 5678")).toBe(true);
    expect(hit(other, "1234, 5678")).toBe(false);
    expect(hit(timeout, "1234,5678")).toBe(true);
  });

  test("within one alternative every word must still match", () => {
    expect(matchesQuery(login, "login error, timeout")).toBe(true);
    expect(matchesQuery(timeout, "login error, timeout")).toBe(true);
    expect(matchesQuery(["Login page"], "login error, timeout")).toBe(false);
  });

  test("a number field still matches #123 inside an alternative", () => {
    expect(matchesQuery(timeout, "#1234, #5678")).toBe(true);
    expect(matchesQuery(other, "#1234, #5678")).toBe(false);
  });

  test("a comma inside quotes is a literal character", () => {
    expect(matchesQuery(["Hello, world"], '"hello, world"')).toBe(true);
    expect(matchesQuery(["Hello world"], '"hello, world"')).toBe(false);
    expect(matchesQuery(["Hello, world"], '"hello, world", nothing')).toBe(true);
  });

  test("empty alternatives are ignored; only commas and spaces match everything", () => {
    expect(matchesQuery(login, ",,")).toBe(true);
    expect(matchesQuery(login, " , ,  ")).toBe(true);
    expect(matchesQuery(login, "nothing,")).toBe(false);
    expect(matchesQuery(login, ",,smoke,")).toBe(true);
    expect(matchesQuery(login, "nothing,,")).toBe(false);
  });
});

describe("searchAlternatives", () => {
  test("a lone quote does not break the split: the rest is one alternative", () => {
    expect(searchAlternatives('"a, b')).toEqual(['"a, b']);
    expect(matchesQuery(["a, b"], '"a, b')).toBe(true);
    expect(matchesQuery(["x"], '", x')).toBe(false);
  });

  test("splits on commas outside quotes, trimmed, empties dropped", () => {
    expect(searchAlternatives("1234, 5678")).toEqual(["1234", "5678"]);
    expect(searchAlternatives(" login error ,, timeout, ")).toEqual(["login error", "timeout"]);
    expect(searchAlternatives('"a, b", c')).toEqual(['"a, b"', "c"]);
    expect(searchAlternatives(" , ")).toEqual([]);
    expect(searchAlternatives("")).toEqual([]);
  });

  test("removes repeated alternatives", () => {
    expect(searchAlternatives("a, A, a")).toEqual(["a"]);
  });
});
