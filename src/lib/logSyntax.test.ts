import { expect, test } from "vitest";
import { LOG_KIND_CLASS, levelOf, tokenizeLog } from "./logSyntax";

const kinds = (s: string) => tokenizeLog(s).filter((t) => t.kind !== "plain").map((t) => [t.kind, t.text]);

test("a request line colours the host, the ids, the status and the timing", () => {
  expect(kinds("GET dev.azure.com/PeoplesHR/HRM/_apis/testplan/Plans/107281/suites -> 200 in 184 ms")).toEqual([
    ["host", "dev.azure.com"],
    ["number", "107281"],
    ["arrow", "->"],
    ["number", "200"],
    ["number", "184"],
  ]);
});

test("the plain text between tokens is kept exactly", () => {
  const line = "POST dev.azure.com/PeoplesHR/_apis/wit/$batch -> 200 in 1683 ms";
  expect(tokenizeLog(line).map((t) => t.text).join("")).toBe(line);
});

test("an error line colours the level word and the status, not the quoted title", () => {
  expect(
    kinds("Submit failed for 'Manage Orders for Team | The Visitor Orders tab': Azure DevOps returned HTTP 400"),
  ).toEqual([["number", "400"]]);
  expect(kinds("ERROR while saving, falling back")).toEqual([["error", "ERROR"]]);
});

test("dates, GUIDs and constants are their own kinds", () => {
  expect(kinds("run 3f2504e0-4f89-11d3-9a0c-0305e82c3301 at 2026-09-11 04:11:21 ok=true")).toEqual([
    ["guid", "3f2504e0-4f89-11d3-9a0c-0305e82c3301"],
    ["date", "2026-09-11 04:11:21"],
    ["constant", "true"],
  ]);
});

test("digits inside words, ids with a hash and version numbers are handled sensibly", () => {
  // Part of a word is not a number.
  expect(kinds("api7 v2beta")).toEqual([]);
  // A PBI id reads as a number; a version as one number, not two.
  expect(kinds("PBI #151632 in batches of 25, version 1.25.5")).toEqual([
    ["number", "151632"],
    ["number", "25"],
    ["number", "1.25.5"],
  ]);
});

test("level words map to the four levels, whatever their case", () => {
  expect(levelOf("ERROR")).toBe("error");
  expect(levelOf("warning")).toBe("warn");
  expect(levelOf("Info")).toBe("info");
  expect(levelOf("trace")).toBe("debug");
  expect(levelOf("submit")).toBeNull();
});

test("every kind has a theme colour, and none is a raw colour", () => {
  for (const cls of Object.values(LOG_KIND_CLASS)) {
    expect(cls).toMatch(/^text-(text|accent|muted|faint|danger|warning|success)(\/\d+)?$/);
  }
});
