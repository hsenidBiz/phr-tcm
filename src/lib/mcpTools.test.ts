import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  API_TEMPLATE_TOOLS,
  AUTORUN_TOOLS,
  CORE_TOOLS,
  CRYSTAL_TOOLS,
  DB_TOOLS,
  dbReadAccessOn,
  disabledToolsSnapshot,
  loadDisabledTools,
  saveDisabledTools,
  SHARED_TOOLS,
  MCP_TOOLS,
  moduleOfTool,
  subscribeDisabledTools,
  toggleRow,
  toggleTool,
  toolOffered,
  toolShown,
  visibleRows,
  visibleTools,
} from "./mcpTools";
import { ALL_MODULES, resetModulesStore, setModule, setModulesForTest } from "./modules";

// The capture flag is a bare localStorage key this file's own tests set and
// clear by hand (no shared afterEach existed here before); a test that
// throws before its own removeItem would otherwise leak it into every test
// that runs after it in the same file.
// A fresh store has no module on. Most tests here want the tool rows of every
// module, as a machine with everything switched on shows them; the ones that
// are about a module being off set their own.
beforeEach(() => {
  setModulesForTest(ALL_MODULES);
});

afterEach(() => {
  localStorage.removeItem("tcm-v2-dev-capture");
  resetModulesStore();
  clearMocks();
});

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

/// Past one page the list is an index, not every template in full: the
/// summary says how it is narrowed and paged.
test("the template list's summary describes its filters and paging", () => {
  const list = MCP_TOOLS.find((t) => t.name === "list_api_templates");
  expect(list?.summary).toBe(
    "This project's saved templates, filtered by module, search, flow or id and paged: in full when one page holds them, otherwise a compact index.",
  );
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
test("core tools are never listed, whatever is switched on", () => {
  const listed = visibleTools().map((t) => t.name);
  for (const name of CORE_TOOLS) {
    expect(listed).not.toContain(name);
  }
  // The switchable ones are still all there - this must not empty the list.
  expect(listed).toContain("search_wiki");
  expect(listed).toContain("get_tags");
  // Every module is on here, so nothing but the core tools is missing.
  expect(listed.length).toBe(MCP_TOOLS.length - CORE_TOOLS.length);
});

test("a saved list naming a core tool is ignored on load", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_test_cases", "get_tags"]));
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/**
 * CORE_TOOLS and each module's tool list are hand-written twice - once in
 * `ai_tools.rs`, once here - and nothing else keeps them in sync. Read the
 * Rust consts directly and compare, so a change on one side that forgets
 * the other fails a test instead of drifting quietly.
 */
test("the core and per-module tool lists match the Rust side", () => {
  const rs = readFileSync(resolve(process.cwd(), "src-tauri/src/ai_tools.rs"), "utf8");

  const extractList = (constName: string): string[] => {
    const start = rs.indexOf(`pub const ${constName}: &[&str] = &[`);
    expect(start).toBeGreaterThan(-1);
    const end = rs.indexOf("];", start);
    expect(end).toBeGreaterThan(start);
    const slice = rs.slice(start, end);
    return [...slice.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  };

  // One list per module on each side: moduleOfTool mirrors module_of_tool.
  expect([...extractList("CORE_TOOLS")].sort()).toEqual([...CORE_TOOLS].sort());
  expect(extractList("AUTORUN_TOOLS")).toEqual([...AUTORUN_TOOLS]);
  expect(extractList("API_TEMPLATE_TOOLS")).toEqual([...API_TEMPLATE_TOOLS]);
  expect(extractList("DB_TOOLS")).toEqual([...DB_TOOLS]);
  expect(extractList("CRYSTAL_TOOLS")).toEqual([...CRYSTAL_TOOLS]);
  for (const name of extractList("AUTORUN_TOOLS")) expect(moduleOfTool(name)).toBe("auto_run");
  for (const name of extractList("API_TEMPLATE_TOOLS")) expect(moduleOfTool(name)).toBe("api_templates");
  for (const name of extractList("DB_TOOLS")) expect(moduleOfTool(name)).toBe("db_access");
  for (const name of extractList("CRYSTAL_TOOLS")) expect(moduleOfTool(name)).toBe("crystal_reports");
  for (const name of extractList("CORE_TOOLS")) expect(moduleOfTool(name)).toBeNull();
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

/// get_run_failures became get_run_results. Someone who had switched it off
/// had refused an assistant their run results - the new name stays off.
test("a tool switched off under its old name stays off under its new one", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_run_failures", "get_tags"]));
  expect([...loadDisabledTools()].sort()).toEqual(["get_run_results", "get_tags"]);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_run_failures", "get_run_results"]));
  expect(loadDisabledTools()).toEqual(["get_run_results"]);
  localStorage.clear();
});

/// The quirk tool records and retires for both the Auto Run and the API
/// templates assistant, so it rides in both rows and is off only while
/// every row carrying it is off - a row whose module is off counts as off.
test("the quirk tool rides in both rows and is off only while both are off", () => {
  expect(SHARED_TOOLS).toEqual(["record_quirk"]);
  const autorun = visibleRows().find((r) => r.label === "Auto Run scripts")!.names;
  const api = visibleRows().find((r) => r.label === "API templates")!.names;
  expect(autorun).toContain("record_quirk");
  expect(api).toContain("record_quirk");

  // A click on a row, as the screen makes it: on the list as loaded.
  const click = (names: string[]) => saveDisabledTools(toggleRow(loadDisabledTools(), names));

  // The Auto Run row off, the API templates row on: still offered.
  click(autorun);
  expect(loadDisabledTools()).not.toContain("record_quirk");
  // Both rows off: off.
  click(api);
  expect(loadDisabledTools()).toContain("record_quirk");
  // The API templates row on again: offered again.
  click(api);
  expect(loadDisabledTools()).not.toContain("record_quirk");
  // The Auto Run row on again: both on, and every Auto Run tool with it.
  click(autorun);
  expect(loadDisabledTools()).toEqual([]);

  // With API Templates off its row counts as off: the Auto Run row decides.
  setModulesForTest(["db_access", "auto_run"]);
  click(autorun);
  expect(loadDisabledTools()).toContain("record_quirk");
  click(autorun);
  expect(loadDisabledTools()).not.toContain("record_quirk");
  localStorage.clear();
});

/// The four quirk tools became one. A row switched off under the old names
/// stays off under the new one.
test("the old quirk tools' names switched off carry over to the one quirk tool", () => {
  setModulesForTest(["db_access", "auto_run"]);
  const own = AUTORUN_TOOLS.filter((n) => n !== "record_quirk");
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...own, "record_autorun_quirk", "retire_autorun_quirk"]));
  const loaded = loadDisabledTools();
  expect(loaded).toContain("record_quirk");
  for (const old of ["record_autorun_quirk", "retire_autorun_quirk", "record_app_quirk", "retire_app_quirk"]) {
    expect(loaded).not.toContain(old);
  }
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

/// With every module on, the Auto Run pair is one of the rows too. The
/// suite pair takes one switch, like the wiki pair: a reader that can never
/// be handed a suite id is no choice at all.
test("only the switchable tools are left, as nine rows with every module on", () => {
  expect(visibleRows().map((r) => r.key)).toEqual([
    "search_test_suites+get_suite_test_cases",
    "get_run_results",
    "save_autorun_script+get_autorun_script+delete_autorun_script+get_autorun_page+probe_autorun_locator+try_autorun_action+replay_autorun_to_step+stop_autorun_replay+start_autorun_discovery+discover_autorun_action+discover_autorun_actions+save_autorun_area+end_autorun_discovery+release_autorun_browser+reset_autorun_browser+save_autorun_component+remove_autorun_component+get_autorun_failures+record_quirk+record_validation_rule+mark_autorun_suspected_defect+set_autorun_order+propose_accounts+get_accounts+list_test_files+get_autorun_network_wait+list_autorun_requests+search_autorun_requests+get_autorun_request+get_autorun_response+get_autorun_response_body+set_autorun_capture+get_autorun_capture+export_autorun_requests+import_autorun_requests+set_autorun_headers+get_autorun_cookies+set_autorun_cookie+clear_autorun_cookies+inspect_autorun_element",
    "list_api_templates+prove_api_template+run_api_template+save_api_flow+get_api_flow_progress+save_api_fixture+run_api_fixture+list_api_fixtures+record_quirk",
    "db_lookup+db_query",
    "open_report+read_report+edit_report+compare_reports+check_report+new_report+preview_report+preview_report_live",
    "get_tags",
    "search_pbis",
    "search_wiki+get_wiki_page",
  ]);
  const suites = visibleRows().find((r) => r.key.startsWith("search_test_suites"));
  expect(suites?.label).toBe("Test Suites");
  const autorun = visibleRows().find((r) => r.key.startsWith("save_autorun_script"));
  expect(autorun?.label).toBe("Auto Run scripts");
});

/// Reading the database is one choice - find the table, read it - so it is
/// one row and one switch, and neither tool is core: the Database Access
/// module owns the row.
test("the two database tools are one switchable row, there while Database Access is on", () => {
  const row = visibleRows().find((r) => r.names.includes("db_lookup"));
  expect(row, "the database row exists").toBeTruthy();
  expect(row!.names).toEqual(["db_lookup", "db_query"]);
  expect(row!.label).toBe("Database Read Access");

  const off = toggleRow([], row!.names);
  expect([...off].sort()).toEqual(["db_lookup", "db_query"]);
  expect(toggleRow(off, row!.names)).toEqual([]);

  // A list saved before the pairing naming one half completes toward OFF.
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query"]));
  expect([...loadDisabledTools()].sort()).toEqual(["db_lookup", "db_query"]);
  localStorage.clear();

  setModulesForTest([]);
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(false);
});

/// With Auto Run on, its group is offered as one row and its switch moves
/// every tool in it together, the same as any other pair.
test("the Auto Run scripts row carries all forty tools", () => {
  const row = visibleRows().find((r) => r.label === "Auto Run scripts");
  expect(row, "the Auto Run scripts row exists").toBeTruthy();
  expect(row!.names).toEqual([
    "save_autorun_script",
    "get_autorun_script",
    "delete_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "replay_autorun_to_step",
    "stop_autorun_replay",
    "start_autorun_discovery",
    "discover_autorun_action",
    "discover_autorun_actions",
    "save_autorun_area",
    "end_autorun_discovery",
    "release_autorun_browser",
    "reset_autorun_browser",
    "save_autorun_component",
    "remove_autorun_component",
    "get_autorun_failures",
    "record_quirk",
    "record_validation_rule",
    "mark_autorun_suspected_defect",
    "set_autorun_order",
    "propose_accounts",
    "get_accounts",
    "list_test_files",
    "get_autorun_network_wait",
    "list_autorun_requests",
    "search_autorun_requests",
    "get_autorun_request",
    "get_autorun_response",
    "get_autorun_response_body",
    "set_autorun_capture",
    "get_autorun_capture",
    "export_autorun_requests",
    "import_autorun_requests",
    "set_autorun_headers",
    "get_autorun_cookies",
    "set_autorun_cookie",
    "clear_autorun_cookies",
    "inspect_autorun_element",
  ]);
  for (const name of row!.names) expect(AUTORUN_TOOLS as readonly string[]).toContain(name);

  const off = toggleRow([], row!.names);
  expect([...off].sort()).toEqual([...row!.names].sort());
  expect(toggleRow(off, row!.names)).toEqual([]);
});

/// Building an API template is one job - guide, list, prove, run, the
/// flow it belongs to and the fixtures built from it - and the quirks it learns - so its nine tools are one row with one switch, offered while API Templates is on.
/// The quirk tool is Auto Run's, shared with this row: off only while both rows are.
test("the API templates row carries all nine tools and switches them together", () => {
  const row = visibleRows().find((r) => r.label === "API templates");
  expect(row, "the API templates row exists").toBeTruthy();
  expect(row!.names).toEqual([
    "list_api_templates",
    "prove_api_template",
    "run_api_template",
    "save_api_flow",
    "get_api_flow_progress",
    "save_api_fixture",
    "run_api_fixture",
    "list_api_fixtures",
    "record_quirk",
  ]);
  expect(row!.summary).toBe(
    "Map a module's stages, then build, prove and run templates that write test data through the application's own endpoints, in the order the application allows.",
  );
  for (const name of row!.names) expect([...API_TEMPLATE_TOOLS, ...SHARED_TOOLS]).toContain(name);

  const off = toggleRow([], row!.names);
  expect([...off].sort()).toEqual([...row!.names].sort());
  expect(toggleRow(off, row!.names)).toEqual([]);

  // A list naming one half completes toward OFF, like every other group -
  // all but the quirk tool, which the Auto Run row (on) still carries.
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["prove_api_template"]));
  expect([...loadDisabledTools()].sort()).toEqual([...API_TEMPLATE_TOOLS].sort());
  localStorage.clear();
});

/// Working on a report is one job - open it, read it, change it, check it,
/// see it - so its eight tools are one row with one switch, offered while
/// Crystal Reports is on. Its guide is Core's get_guide.
test("the Crystal Reports row carries all eight tools and follows its module", () => {
  const row = visibleRows().find((r) => r.label === "Crystal Reports");
  expect(row, "the Crystal Reports row exists").toBeTruthy();
  expect(row!.names).toEqual([
    "open_report",
    "read_report",
    "edit_report",
    "compare_reports",
    "check_report",
    "new_report",
    "preview_report",
    "preview_report_live",
  ]);
  for (const name of row!.names) {
    expect(CRYSTAL_TOOLS as readonly string[]).toContain(name);
    expect(moduleOfTool(name)).toBe("crystal_reports");
  }

  const off = toggleRow([], row!.names);
  expect([...off].sort()).toEqual([...row!.names].sort());
  expect(toggleRow(off, row!.names)).toEqual([]);

  // A list naming one half completes toward OFF, like every other group.
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["edit_report"]));
  expect([...loadDisabledTools()].sort()).toEqual([...row!.names].sort());
  localStorage.clear();

  setModulesForTest(["db_access", "writing_style", "auto_run", "api_templates", "playwright"]);
  expect(visibleRows().some((r) => r.label === "Crystal Reports")).toBe(false);
  for (const name of CRYSTAL_TOOLS) expect(toolOffered(name)).toBe(false);
});

/// A module that is off takes its tools with it: the row disappears
/// entirely and a list saved while the module was on cannot carry its names
/// into requests, where they would be meaningless.
test("with Auto Run and API Templates off, no row mentions them and a saved list is stripped of their tools", () => {
  setModulesForTest(["db_access", "writing_style"]);

  expect(visibleRows().some((r) => r.label.includes("Auto Run"))).toBe(false);
  expect(visibleRows().some((r) => r.label === "API templates")).toBe(false);
  for (const name of [...AUTORUN_TOOLS, ...API_TEMPLATE_TOOLS]) {
    expect(visibleRows().flatMap((r) => r.names)).not.toContain(name);
  }

  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS, ...API_TEMPLATE_TOOLS, "get_tags"]));
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/// Each module owns its row.
test("each module's row follows its own module", () => {
  setModulesForTest(["db_access", "auto_run"]);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(true);
  expect(visibleRows().some((r) => r.label === "API templates")).toBe(false);
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(true);

  setModulesForTest([]);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(false);
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(false);
  // Core and the Azure DevOps reading tools are always there.
  expect(visibleRows().some((r) => r.key === "search_wiki+get_wiki_page")).toBe(true);
});

test("a tool's module is the mirror of Rust's module_of_tool", () => {
  expect(moduleOfTool("save_autorun_script")).toBe("auto_run");
  expect(moduleOfTool("run_api_template")).toBe("api_templates");
  expect(moduleOfTool("db_query")).toBe("db_access");
  expect(moduleOfTool("edit_report")).toBe("crystal_reports");
  expect(moduleOfTool("get_tags")).toBeNull();
  expect(moduleOfTool("get_test_cases")).toBeNull();
});

/// A saved list that refused database reading survives the module being
/// switched off and on: dropping it would quietly re-enable db_query.
test("a saved list naming the database tools is kept while Database Access is off", () => {
  setModulesForTest([]);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query"]));
  expect([...loadDisabledTools()].sort()).toEqual(["db_lookup", "db_query"]);
  localStorage.clear();
});

test("Database Read Access follows its module and the person's switch", () => {
  setModulesForTest([]);
  expect(dbReadAccessOn()).toBe(false);
  setModulesForTest(["db_access"]);
  expect(dbReadAccessOn()).toBe(true);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query"]));
  expect(dbReadAccessOn()).toBe(false);
  localStorage.clear();
});

/// The screen shows names people read, not identifiers. An underscore in a
/// label is the identifier leaking back out.
test("every row label is human, with no identifier in it", () => {
  for (const row of visibleRows()) {
    expect(row.label, `${row.key} label`).not.toMatch(/_/);
    expect(row.label[0]).toMatch(/[A-Z]/);
  }
});

/// Capture mode reports every module on, but the AI Tools tab is a
/// documented, captured screen: it hides the Auto Run and API template
/// tools in a shot (display only).
test("capture mode hides the Auto Run and API template rows and tools, everything on", () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");

  expect(toolShown("save_autorun_script")).toBe(false);
  expect(toolShown("list_api_templates")).toBe(false);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(false);
  expect(visibleRows().some((r) => r.label === "API templates")).toBe(false);
  expect(visibleTools().some((t) => moduleOfTool(t.name) === "auto_run" || moduleOfTool(t.name) === "api_templates")).toBe(false);
  // The database row is documented and stays.
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(true);

  localStorage.removeItem("tcm-v2-dev-capture");
  expect(toolShown("save_autorun_script")).toBe(true);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(true);
});

/// Capture mode must never put anything on disk: `toolOffered` feeds
/// loadDisabledTools(), snapshotKeyFor() and so AiBridge's `register` ->
/// `commands.registerAiTool(..., loadDisabledTools(), ...)`. It is not
/// narrowed by the display gate, so a saved list keeps the tools the person
/// had switched off.
test("capture mode does not narrow toolOffered() or what loadDisabledTools() keeps", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS, "get_tags"]));
  localStorage.setItem("tcm-v2-dev-capture", "on");

  expect(toolOffered("save_autorun_script")).toBe(true);
  expect(toolShown("save_autorun_script")).toBe(false);
  // All but the quirk tool, which the API templates row (on) still carries.
  expect(loadDisabledTools().sort()).toEqual([...AUTORUN_TOOLS.filter((n) => n !== "record_quirk"), "get_tags"].sort());

  localStorage.removeItem("tcm-v2-mcp-disabled");
});

/// Switching a module fires the disabled-tools subscription, so App
/// re-pushes the bridge context, and changes which saved names count.
test("switching a module re-fires the subscription and changes which saved names count", async () => {
  setModulesForTest([]);
  mockIPC((cmd) => (cmd === "modules_set" ? ["db_access", "auto_run"] : null));

  expect(toolOffered("save_autorun_script")).toBe(false);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(false);

  const fired = vi.fn();
  const off = subscribeDisabledTools(fired);
  await setModule("auto_run", true);
  expect(fired).toHaveBeenCalled();
  expect(toolOffered("save_autorun_script")).toBe(true);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(true);

  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS]));
  expect([...disabledToolsSnapshot()].sort()).toEqual([...AUTORUN_TOOLS].sort());
  mockIPC((cmd) => (cmd === "modules_set" ? [] : null));
  await setModule("db_access", false);
  expect(disabledToolsSnapshot()).toEqual([]);

  off();
  localStorage.clear();
});

/// Off hides, never deletes: saving the list while a module is off (a toggle
/// of some other tool) must not erase that module's saved names - they
/// would come back ENABLED when the module does.
test("saving the list while a module is off keeps that module's saved names", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS]));
  setModulesForTest(["db_access"]);
  // What the screen sees and saves: the read side hides Auto Run's names.
  expect(loadDisabledTools()).toEqual([]);
  saveDisabledTools(toggleRow(loadDisabledTools(), ["search_wiki", "get_wiki_page"]));
  setModulesForTest(["db_access", "auto_run"]);
  expect(AUTORUN_TOOLS.every((n) => loadDisabledTools().includes(n))).toBe(true);
  expect(loadDisabledTools()).toContain("search_wiki");

  // And "Turn all on" (saving nothing) while it is off keeps them too.
  setModulesForTest(["db_access"]);
  saveDisabledTools([]);
  setModulesForTest(["db_access", "auto_run"]);
  expect(AUTORUN_TOOLS.every((n) => loadDisabledTools().includes(n))).toBe(true);
  localStorage.clear();
});

/// toolOffered feeds what is registered with real assistants: it reads the
/// real modules, not capture mode's all-on view of them.
test("in capture mode with no module really on, toolOffered is false and saved names are dropped", () => {
  setModulesForTest([]);
  localStorage.setItem("tcm-v2-dev-capture", "on");
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS, "get_tags"]));
  expect(toolOffered("save_autorun_script")).toBe(false);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  // The screen's display view is the capture one: database row shown.
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(true);
});
