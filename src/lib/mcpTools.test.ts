import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { expect, test, vi } from "vitest";
import { CORE_TOOLS, DEV_BUILD, DEV_ONLY_TOOLS, loadDisabledTools, MCP_TOOLS, toggleRow, toggleTool, visibleRows, visibleTools } from "./mcpTools";

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
/// was a control that did nothing, and the eight core tools are enforced
/// on the Rust side whatever this list shows - so they are never offered
/// here, in either build kind.
test("core tools are never listed, in either build kind", () => {
  const listed = visibleTools().map((t) => t.name);
  for (const name of CORE_TOOLS) {
    expect(listed).not.toContain(name);
  }
  // The switchable ones are still all there - this must not empty the list.
  expect(listed).toContain("search_wiki");
  expect(listed).toContain("get_tags");
  // The two dev-only tools are part of the count only when this build
  // offers them at all - see the DEV-stubbed tests below for both values.
  expect(listed.length).toBe(
    MCP_TOOLS.length - CORE_TOOLS.length - (DEV_BUILD ? 0 : DEV_ONLY_TOOLS.length),
  );
});

test("a saved list naming a core tool is ignored on load", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_test_cases", "get_tags"]));
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/**
 * CORE_TOOLS and DEV_ONLY_TOOLS are each hand-written twice - once in
 * `ai_tools.rs`, once here - and nothing else keeps them in sync. Read the
 * Rust consts directly and compare, so a change on one side that forgets
 * the other fails a test instead of drifting quietly.
 */
test("the core and dev-only tool lists match the Rust side", () => {
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
  const rsDevOnly = extractList("DEV_ONLY_TOOLS");

  expect([...rsCore].sort()).toEqual([...CORE_TOOLS].sort());
  expect([...rsDevOnly].sort()).toEqual([...DEV_ONLY_TOOLS].sort());
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

/// This suite (and this file's default vitest env) runs as a development
/// build - DEV_BUILD is true unless a test stubs it otherwise - so the
/// Auto Run pair is one of the rows here too. The suite pair takes one
/// switch, like the wiki pair: a reader that can never be handed a suite
/// id is no choice at all.
test("only the switchable tools are left, as seven rows in a development build", () => {
  expect(DEV_BUILD, "this file's default env").toBe(true);
  expect(visibleRows().map((r) => r.key)).toEqual([
    "search_test_suites+get_suite_test_cases",
    "get_run_failures",
    "get_autorun_guide+save_autorun_script+get_autorun_page+probe_autorun_locator+try_autorun_action+get_autorun_failures+record_autorun_quirk",
    "db_lookup+db_query",
    "get_tags",
    "search_pbis",
    "search_wiki+get_wiki_page",
  ]);
  const suites = visibleRows().find((r) => r.key.startsWith("search_test_suites"));
  expect(suites?.label).toBe("Test Suites");
  const autorun = visibleRows().find((r) => r.key.startsWith("get_autorun_guide"));
  expect(autorun?.label).toBe("Auto Run scripts");
});

/// Reading the database is one choice - find the table, read it - so it is
/// one row and one switch, and neither tool is core or development-only:
/// a release build offers the row exactly as this one does.
test("the two database tools are one switchable row in either build kind", async () => {
  const row = visibleRows().find((r) => r.names.includes("db_lookup"));
  expect(row, "the database row exists").toBeTruthy();
  expect(row!.names).toEqual(["db_lookup", "db_query"]);
  expect(row!.label).toBe("Company database (read)");

  const off = toggleRow([], row!.names);
  expect([...off].sort()).toEqual(["db_lookup", "db_query"]);
  expect(toggleRow(off, row!.names)).toEqual([]);

  // A list saved before the pairing naming one half completes toward OFF.
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query"]));
  expect([...loadDisabledTools()].sort()).toEqual(["db_lookup", "db_query"]);
  localStorage.clear();

  vi.stubEnv("DEV", false);
  vi.resetModules();
  const mod = await import("./mcpTools");
  expect(mod.visibleRows().some((r) => r.label === "Company database (read)")).toBe(true);
  vi.unstubAllEnvs();
  vi.resetModules();
});

/// With DEV stubbed true, the Auto Run group is offered as one row and its
/// switch moves every tool in it together, the same as any other pair.
test("with DEV stubbed true, the Auto Run scripts row carries all seven tools", async () => {
  vi.stubEnv("DEV", true);
  vi.resetModules();
  const mod = await import("./mcpTools");

  const row = mod.visibleRows().find((r) => r.label === "Auto Run scripts");
  expect(row, "the Auto Run scripts row exists").toBeTruthy();
  expect(row!.names).toEqual([
    "get_autorun_guide",
    "save_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "get_autorun_failures",
    "record_autorun_quirk",
  ]);

  const off = mod.toggleRow([], row!.names);
  expect([...off].sort()).toEqual([...row!.names].sort());
  expect(mod.toggleRow(off, row!.names)).toEqual([]);

  vi.unstubAllEnvs();
  vi.resetModules();
});

/// With DEV stubbed false, the row disappears entirely and a list saved
/// while the app was built for development cannot carry the two names
/// into a release build's requests, where they would be meaningless.
test("with DEV stubbed false, no row mentions Auto Run and a saved list is stripped of it", async () => {
  vi.stubEnv("DEV", false);
  vi.resetModules();
  const mod = await import("./mcpTools");

  expect(mod.visibleRows().some((r) => r.label.includes("Auto Run"))).toBe(false);
  for (const name of mod.DEV_ONLY_TOOLS) {
    expect(mod.visibleRows().flatMap((r) => r.names)).not.toContain(name);
  }

  localStorage.setItem(
    "tcm-v2-mcp-disabled",
    JSON.stringify([...mod.DEV_ONLY_TOOLS, "get_tags"]),
  );
  expect(mod.loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();

  vi.unstubAllEnvs();
  vi.resetModules();
});

/// The screen shows names people read, not identifiers. An underscore in a
/// label is the identifier leaking back out.
test("every row label is human, with no identifier in it", () => {
  for (const row of visibleRows()) {
    expect(row.label, `${row.key} label`).not.toMatch(/_/);
    expect(row.label[0]).toMatch(/[A-Z]/);
  }
});

/// A release build whose optional extras are unlocked offers the Auto Run
/// row like a development build does, and a saved list naming its tools is
/// kept while unlocked and stripped again once relocked. App re-pushes the
/// bridge context off `subscribeDisabledTools`, so it must fire on either.
test("with DEV stubbed false, unlocking this machine's extras brings the Auto Run row back", async () => {
  vi.stubEnv("DEV", false);
  vi.resetModules();
  const mod = await import("./mcpTools");
  const extras = await import("./extras");
  mockIPC(() => null);

  expect(mod.autoRunToolsOffered()).toBe(false);
  expect(mod.visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(false);

  const fired = vi.fn();
  const off = mod.subscribeDisabledTools(fired);
  await extras.setExtrasUnlocked(true);
  expect(fired).toHaveBeenCalled();
  expect(mod.autoRunToolsOffered()).toBe(true);
  expect(mod.visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(true);

  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...mod.DEV_ONLY_TOOLS]));
  expect([...mod.disabledToolsSnapshot()].sort()).toEqual([...mod.DEV_ONLY_TOOLS].sort());
  await extras.setExtrasUnlocked(false);
  expect(mod.disabledToolsSnapshot()).toEqual([]);

  off();
  localStorage.clear();
  clearMocks();
  vi.unstubAllEnvs();
  vi.resetModules();
});
