import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { CaseScript } from "../../bindings";
import { uncheckedExpectations, uncheckedLine, uncheckedText, uncheckedWords } from "./unchecked";

const FIXTURE = join(
  dirname(fileURLToPath(import.meta.url)),
  "../../../src-tauri/tests/fixtures/unchecked_expectations.json",
);
const table = JSON.parse(readFileSync(FIXTURE, "utf-8")) as {
  name: string;
  script: CaseScript;
  lines: string[];
  words: string;
}[];

describe("uncheckedExpectations", () => {
  it("counts as the shared fixture says", () => {
    expect(table.length).toBeGreaterThan(5);
    for (const row of table) {
      const found = uncheckedExpectations(row.script);
      expect([row.name, found.map(uncheckedLine)]).toEqual([row.name, row.lines]);
      expect([row.name, found.length === 0 ? "" : uncheckedWords(found.length)]).toEqual([row.name, row.words]);
    }
  });

  it("says one expectation in the singular and the rest in the plural", () => {
    expect(uncheckedWords(1)).toBe("1 expectation unchecked");
    expect(uncheckedWords(3)).toBe("3 expectations unchecked");
  });
});

describe("uncheckedText", () => {
  it("gives a sentence as it stands, trimmed", () => {
    expect(uncheckedText("  a  ")).toBe("a");
  });

  it("gives the entries as item - why, one after another", () => {
    expect(
      uncheckedText([
        { item: "no lock icon", why: "why one" },
        { item: "the click", why: "why two", stand_in: "expect_hidden" },
      ]),
    ).toBe("no lock icon - why one; the click - why two");
  });

  it("gives null for none, an empty list or a blank sentence", () => {
    expect(uncheckedText([])).toBeNull();
    expect(uncheckedText("")).toBeNull();
    expect(uncheckedText("   ")).toBeNull();
    expect(uncheckedText(null)).toBeNull();
    expect(uncheckedText(undefined)).toBeNull();
  });
});
