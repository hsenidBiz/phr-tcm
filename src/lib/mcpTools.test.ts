import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { CORE_TOOLS, HIDDEN_TOOLS, loadDisabledTools, MCP_TOOLS, toggleRow, toggleTool, visibleRows, visibleTools } from "./mcpTools";

/**
 * The toggle list is a hand-written mirror of `mcp.rs`. If the two drift, a
 * newly added tool has no switch in the AI Bridge screen and quietly cannot
 * be turned off - which is the opposite of what the toggles are for. So the
 * source of truth is read here and compared by name.
 */
test("every tool the MCP server advertises has a switch in the UI", () => {
  const rs = readFileSync(resolve(process.cwd(), "src-tauri/src/mcp.rs"), "utf8");
  const listStart = rs.indexOf("fn tools_list");
  expect(listStart).toBeGreaterThan(-1);

  const advertised = [...rs.slice(listStart).matchAll(/"name":\s*"([a-z_]+)"/g)].map(
    (m) => m[1],
  );
  expect(advertised.length).toBeGreaterThan(0);

  const known = MCP_TOOLS.map((t) => t.name);
  expect([...advertised].sort()).toEqual([...known].sort());
});

test("every tool carries a summary the settings screen can show", () => {
  for (const t of MCP_TOOLS) expect(t.summary.trim()).not.toBe("");
});

test("toggling is its own inverse", () => {
  const once = toggleTool([], "search_wiki");
  expect(once).toEqual(["search_wiki"]);
  expect(toggleTool(once, "search_wiki")).toEqual([]);
});

test("core tools cannot be toggled", () => {
  for (const name of CORE_TOOLS) expect(toggleTool([], name)).toEqual([]);
});

/// The list is the tools you can DO something about. A row with no switch
/// was a control that did nothing, and the five it named are enforced on
/// the Rust side whatever this list shows - so they are not offered here
/// at all, the same way the two Auto Run tools already were not.
test("neither core nor hidden tools are listed", () => {
  const listed = visibleTools().map((t) => t.name);
  for (const name of [...CORE_TOOLS, ...HIDDEN_TOOLS]) {
    expect(listed).not.toContain(name);
  }
  // The switchable ones are still all there - this must not empty the list.
  expect(listed).toContain("search_wiki");
  expect(listed).toContain("get_tags");
  expect(listed.length).toBe(MCP_TOOLS.length - CORE_TOOLS.length - HIDDEN_TOOLS.length);
});

test("a saved list naming a core tool is ignored on load", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_test_cases", "get_tags"]));
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/**
 * CORE_TOOLS and HIDDEN_TOOLS are each hand-written twice - once in
 * `ai_tools.rs`, once here - and nothing else keeps them in sync. Read the
 * Rust consts directly and compare, so a change on one side that forgets
 * the other fails a test instead of drifting quietly.
 */
test("the core and hidden tool lists match the Rust side", () => {
  const rs = readFileSync(resolve(process.cwd(), "src-tauri/src/ai_tools.rs"), "utf8");

  const extractList = (constName: string): string[] => {
    const start = rs.indexOf(`pub const ${constName}: &[&str] = &[`);
    expect(start).toBeGreaterThan(-1);
    const end = rs.indexOf("];", start);
    expect(end).toBeGreaterThan(start);
    const slice = rs.slice(start, end);
    return [...slice.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  };

  const rsCore = extractList("CORE_TOOLS");
  const rsHidden = extractList("HIDDEN_TOOLS");

  expect([...rsCore].sort()).toEqual([...CORE_TOOLS].sort());
  expect([...rsHidden].sort()).toEqual([...HIDDEN_TOOLS].sort());
});

/// `get_wiki_page` reads a page that `search_wiki` found - it has no way to
/// name a page on its own. Offered as two switches, half the combinations
/// were useless: search with nothing to read it, or a reader that can never
/// be handed anything. They are one choice, so they are one row.
test("the two wiki tools are one row that carries both names", () => {
  const rows = visibleRows();
  const wiki = rows.find((r) => r.names.includes("search_wiki"));
  expect(wiki, "the wiki row exists").toBeTruthy();
  expect(wiki!.names).toEqual(["search_wiki", "get_wiki_page"]);
  // And neither appears again on its own.
  expect(rows.filter((r) => r.names.includes("get_wiki_page")).length).toBe(1);
  expect(rows.every((r) => r.key !== "get_wiki_page")).toBe(true);
});

test("switching the wiki row off disables both tools, and on clears both", () => {
  const off = toggleRow([], ["search_wiki", "get_wiki_page"]);
  expect([...off].sort()).toEqual(["get_wiki_page", "search_wiki"]);
  expect(toggleRow(off, ["search_wiki", "get_wiki_page"])).toEqual([]);
});

/// The suite pair is one switch too: off disables the search and the case
/// reader together, and a saved list naming one half completes toward OFF.
test("the Test Suites row switches both suite tools together", () => {
  const off = toggleRow([], ["search_test_suites", "get_suite_test_cases"]);
  expect([...off].sort()).toEqual(["get_suite_test_cases", "search_test_suites"]);
  expect(toggleRow(off, ["search_test_suites", "get_suite_test_cases"])).toEqual([]);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_suite_test_cases"]));
  expect([...loadDisabledTools()].sort()).toEqual(["get_suite_test_cases", "search_test_suites"]);
  localStorage.clear();
});

/// A saved list from before the pairing can name one without the other.
/// Completing it toward OFF is the safe direction: the alternative silently
/// hands an assistant a tool the user had switched off.
test("a half-disabled wiki pair is completed on load", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_wiki_page"]));
  expect([...loadDisabledTools()].sort()).toEqual(["get_wiki_page", "search_wiki"]);
  localStorage.clear();
});

/// The three that finish a draft - validate, optimise, merge - are as much
/// part of writing a set as the guide is, and a half-set with them switched
/// off is a set nobody can ship. They joined the always-on group, which
/// also takes them out of the switch list.
test("validate, optimise and merge are always on and not listed", () => {
  for (const name of ["validate_cases", "optimize_cases", "merge_case_files"]) {
    expect(CORE_TOOLS as readonly string[]).toContain(name);
    expect(visibleRows().flatMap((r) => r.names)).not.toContain(name);
  }
});

/// Five rows is the whole of what this screen can still decide. The suite
/// pair takes one switch, like the wiki pair: a reader that can never be
/// handed a suite id is no choice at all.
test("only the switchable tools are left, as five rows", () => {
  expect(visibleRows().map((r) => r.key)).toEqual([
    "search_test_suites+get_suite_test_cases",
    "get_run_failures",
    "get_tags",
    "search_pbis",
    "search_wiki+get_wiki_page",
  ]);
  const suites = visibleRows().find((r) => r.key.startsWith("search_test_suites"));
  expect(suites?.label).toBe("Test Suites");
});

/// The screen shows names people read, not identifiers. An underscore in a
/// label is the identifier leaking back out.
test("every row label is human, with no identifier in it", () => {
  for (const row of visibleRows()) {
    expect(row.label, `${row.key} label`).not.toMatch(/_/);
    expect(row.label[0]).toMatch(/[A-Z]/);
  }
});
