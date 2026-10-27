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
  NETWORK_TOOLS,
  RENAMED_TOOLS,
  clearModuleToolSwitches,
  dbReadAccessOn,
  disabledToolsSnapshot,
  loadDisabledTools,
  saveDisabledTools,
  MCP_TOOLS,
  moduleOfTool,
  subscribeDisabledTools,
  toggleRow,
  toggleTool,
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

/// The AI Bridge tab shows in either mode of "Treat all projects as one", so
/// the Azure DevOps rows read true in both: they name no single project.
test("the suite and wiki summaries name no single project", () => {
  const summary = (name: string) => MCP_TOOLS.find((t) => t.name === name)?.summary;
  expect(summary("search_test_suites")).toBe("The plans and suites, by plan name, suite name or PBI id.");
  expect(summary("search_wiki")).toBe("Search the wikis for documentation.");
  const wiki = visibleRows().find((r) => r.names.includes("search_wiki"));
  expect(wiki?.label).toBe("Project wiki");
  expect(wiki?.summary).toBe("Search the wikis and read the pages they find.");
});

/// Past one page the list is an index, not every template in full: the
/// summary says how it is narrowed and paged.
test("the template list's summary describes its filters and paging", () => {
  const list = MCP_TOOLS.find((t) => t.name === "list_api_templates");
  expect(list?.summary).toBe(
    "This project's saved templates, filtered by module, search, flow or id and paged: in full when one page holds them, otherwise a compact index. Or its fixtures, with their current outputs and last run.",
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
  // Only the core tools and the module tools are missing: a module's tools
  // have no switch, the module being on is the switch.
  const moduleTools = MCP_TOOLS.filter((t) => moduleOfTool(t.name) !== null).length;
  expect(listed.length).toBe(MCP_TOOLS.length - CORE_TOOLS.length - moduleTools);
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
  // The network tools are Auto Run's, behind their own switch besides.
  expect(extractList("NETWORK_TOOLS")).toEqual([...NETWORK_TOOLS]);
  for (const name of NETWORK_TOOLS) expect(AUTORUN_TOOLS as readonly string[]).toContain(name);
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

/// A module's tools have no switch of their own: the module being on in
/// Settings is the switch. So no row on this tab carries one, whichever
/// modules are on, and nothing the tab saves or loads can name one - the
/// quirk tool, which two modules share, included.
test("no row carries a module's tool, and the quirk tool is never on a disabled list", () => {
  const moduleTools = MCP_TOOLS.filter((t) => moduleOfTool(t.name) !== null).map((t) => t.name);
  expect(moduleTools).toContain("record_quirk");
  expect(moduleTools).toContain("db_query");
  expect(moduleTools).toContain("save_autorun_script");
  for (const modules of [ALL_MODULES, [], ["auto_run"], ["api_templates"], ["db_access"]] as const) {
    setModulesForTest([...modules]);
    const listed = visibleRows().flatMap((r) => r.names);
    for (const name of moduleTools) expect(listed, `${name} with ${modules.join("+") || "no module"}`).not.toContain(name);
    expect(visibleRows().some((r) => ["Auto Run scripts", "API templates", "Database Read Access", "Crystal Reports"].includes(r.label))).toBe(false);
  }
  saveDisabledTools(["record_quirk", "get_tags"]);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/// The Auto Run tools merged into one with an argument. A row switched off
/// under the old names used to carry over to the new ones; module tools have
/// no switch now, so the old names drop out with the rest - but every old
/// name still maps to a tool that is there.
test("the merged Auto Run tools' old names switched off no longer name anything", () => {
  setModulesForTest(["db_access", "auto_run"]);
  const merged: Record<string, string> = {
    discover_autorun_action: "discover_autorun_actions",
    inspect_autorun_element: "probe_autorun_locator",
    stop_autorun_replay: "replay_autorun_to_step",
    release_autorun_browser: "reset_autorun_browser",
    list_autorun_requests: "search_autorun_requests",
    get_autorun_response: "get_autorun_request",
    get_autorun_response_body: "get_autorun_request",
    set_autorun_capture: "autorun_capture",
    get_autorun_capture: "autorun_capture",
    export_autorun_requests: "autorun_har",
    import_autorun_requests: "autorun_har",
    get_autorun_cookies: "autorun_cookies",
    set_autorun_cookie: "autorun_cookies",
    clear_autorun_cookies: "autorun_cookies",
  };
  for (const [old, now] of Object.entries(merged)) {
    expect(RENAMED_TOOLS[old]).toBe(now);
    localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([old, "get_tags"]));
    expect(loadDisabledTools()).toEqual(["get_tags"]);
  }
  for (const now of Object.values(RENAMED_TOOLS)) expect(MCP_TOOLS.map((t) => t.name)).toContain(now);
  localStorage.clear();
});

/// The four quirk tools became one, and the quirk tool belongs to the
/// modules: old names for it drop out of a saved list like any module tool.
test("the old quirk tools' names no longer name anything", () => {
  setModulesForTest(["db_access", "auto_run"]);
  localStorage.setItem(
    "tcm-v2-mcp-disabled",
    JSON.stringify(["record_autorun_quirk", "retire_autorun_quirk", "record_app_quirk", "retire_app_quirk", "get_tags"]),
  );
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  localStorage.clear();
});

/// The API templates tools switched off under old names, before the switches
/// went: nothing of them survives a load.
test("a saved list of the API templates tools and the old quirk names loads empty", () => {
  localStorage.setItem(
    "tcm-v2-mcp-disabled",
    JSON.stringify([...API_TEMPLATE_TOOLS, "record_app_quirk", "retire_app_quirk"]),
  );
  expect(loadDisabledTools()).toEqual([]);
  localStorage.clear();
});

/// The four guide tools became Core's get_guide: a saved list naming them
/// drops them as Core, rather than sending names that match nothing.
test("the old guide tools' names are dropped from a saved list", () => {
  localStorage.setItem(
    "tcm-v2-mcp-disabled",
    JSON.stringify(["get_writing_guide", "get_autorun_guide", "get_api_template_guide", "get_report_guide", "get_tags"]),
  );
  expect(loadDisabledTools()).toEqual(["get_tags"]);
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

/// Only the tools that belong to no module have a switch. The suite pair
/// takes one switch, like the wiki pair: a reader that can never be handed a
/// suite id is no choice at all. The rows are the same with every module on
/// as with none.
test("only the switchable tools are left, as five rows whatever modules are on", () => {
  const five = [
    "search_test_suites+get_suite_test_cases",
    "get_run_results",
    "get_tags",
    "search_pbis",
    "search_wiki+get_wiki_page",
  ];
  expect(visibleRows().map((r) => r.key)).toEqual(five);
  setModulesForTest([]);
  expect(visibleRows().map((r) => r.key)).toEqual(five);
  const suites = visibleRows().find((r) => r.key.startsWith("search_test_suites"));
  expect(suites?.label).toBe("Test Suites");
});

/// A module's tools are offered exactly while the module is on. They carry no
/// switch of their own, so a saved list that still names them - from before
/// the switches went - is read as empty of them, whatever modules are on,
/// whichever half of a group it names.
test("a saved list naming a module's tools loads without them", () => {
  const modules: Record<string, readonly string[]> = {
    "Auto Run": AUTORUN_TOOLS,
    "API templates": API_TEMPLATE_TOOLS,
    "Database Access": DB_TOOLS,
    "the held-back module": CRYSTAL_TOOLS,
  };
  for (const [label, tools] of Object.entries(modules)) {
    for (const on of [ALL_MODULES, []]) {
      setModulesForTest([...on]);
      localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...tools, "get_tags"]));
      expect(loadDisabledTools(), `${label}, ${on.length} modules on`).toEqual(["get_tags"]);
      // One tool of a group does not drag the rest of it in either.
      localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([tools[0], "search_pbis"]));
      expect(loadDisabledTools(), `${label}, one tool`).toEqual(["search_pbis"]);
    }
  }
  localStorage.clear();
});

/// A module's tool is offered whenever its module is on, even if an old
/// saved list named it off: nothing reads a module tool's name off the list
/// any more.
test("a database tool an old list switched off is not off once Database Access is on", () => {
  setModulesForTest(["db_access"]);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query", "db_lookup"]));
  expect(disabledToolsSnapshot()).toEqual([]);
  expect(dbReadAccessOn()).toBe(true);
  localStorage.clear();
});

/// A list saved before this change is cleaned in storage, for every
/// environment and for the single list older versions kept: the module
/// tools go, the rest stays as it was, and a list left empty is removed.
test("clearModuleToolSwitches strips module tools from each environment's saved list", () => {
  const base = "tcm-v2-mcp-disabled";
  localStorage.setItem(base, JSON.stringify(["db_query", "get_tags"]));
  localStorage.setItem(`${base}:env-a`, JSON.stringify(["db_query", "save_autorun_script", "get_tags"]));
  localStorage.setItem(`${base}:env-b`, JSON.stringify(["db_lookup", "db_query", ...AUTORUN_TOOLS]));
  localStorage.setItem(`${base}:env-c`, JSON.stringify(["search_wiki", "get_wiki_page"]));
  localStorage.setItem(`${base}:env-d`, JSON.stringify(["record_app_quirk", "get_run_failures"]));
  localStorage.setItem("tcm-v2-db-writes:env-a", "1");

  clearModuleToolSwitches();

  expect(JSON.parse(localStorage.getItem(base)!)).toEqual(["get_tags"]);
  expect(JSON.parse(localStorage.getItem(`${base}:env-a`)!)).toEqual(["get_tags"]);
  // Nothing but module tools: the list goes.
  expect(localStorage.getItem(`${base}:env-b`)).toBeNull();
  // A list of switchable tools only is left alone, byte for byte.
  expect(localStorage.getItem(`${base}:env-c`)).toBe(JSON.stringify(["search_wiki", "get_wiki_page"]));
  // A retired name that now means a module tool goes; one that means a
  // switchable tool carries over under its new name.
  expect(JSON.parse(localStorage.getItem(`${base}:env-d`)!)).toEqual(["get_run_results"]);
  // Other switches are not touched.
  expect(localStorage.getItem("tcm-v2-db-writes:env-a")).toBe("1");
  // Doing it again changes nothing.
  clearModuleToolSwitches();
  expect(JSON.parse(localStorage.getItem(`${base}:env-a`)!)).toEqual(["get_tags"]);
  localStorage.clear();
});

test("clearModuleToolSwitches leaves a list it cannot read alone", () => {
  localStorage.setItem("tcm-v2-mcp-disabled:env-a", "not json");
  localStorage.setItem("tcm-v2-mcp-disabled:env-b", JSON.stringify({ not: "a list" }));
  clearModuleToolSwitches();
  expect(localStorage.getItem("tcm-v2-mcp-disabled:env-a")).toBe("not json");
  expect(localStorage.getItem("tcm-v2-mcp-disabled:env-b")).toBe(JSON.stringify({ not: "a list" }));
  localStorage.clear();
});

/// The module tools are tools the server advertises (so they are still
/// described in the breakdown), and none is a row.
test("every module tool is a tool the server advertises, and none is a row", () => {
  const names = new Set(MCP_TOOLS.map((t) => t.name));
  for (const name of [...AUTORUN_TOOLS, ...API_TEMPLATE_TOOLS, ...DB_TOOLS]) expect(names.has(name), name).toBe(true);
  const rowNames = new Set(visibleRows().flatMap((r) => r.names));
  for (const name of [...AUTORUN_TOOLS, ...API_TEMPLATE_TOOLS, ...DB_TOOLS, ...CRYSTAL_TOOLS]) {
    expect(rowNames.has(name), name).toBe(false);
  }
});

test("a tool's module is the mirror of Rust's module_of_tool", () => {
  expect(moduleOfTool("save_autorun_script")).toBe("auto_run");
  expect(moduleOfTool("run_api_template")).toBe("api_templates");
  expect(moduleOfTool("db_query")).toBe("db_access");
  expect(moduleOfTool("edit_report")).toBe("crystal_reports");
  expect(moduleOfTool("get_tags")).toBeNull();
  expect(moduleOfTool("get_test_cases")).toBeNull();
});

/// Database Read Access is the module being on: the person's old switch for
/// it is gone, and an old saved list naming it changes nothing.
test("Database Read Access follows its module alone", () => {
  setModulesForTest([]);
  expect(dbReadAccessOn()).toBe(false);
  setModulesForTest(["db_access"]);
  expect(dbReadAccessOn()).toBe(true);
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["db_query"]));
  expect(dbReadAccessOn()).toBe(true);
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
/// tools in a shot (display only). No module's tool has a row either way.
test("capture mode hides the Auto Run and API template tools, everything on", () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");

  expect(toolShown("save_autorun_script")).toBe(false);
  expect(toolShown("list_api_templates")).toBe(false);
  expect(visibleTools().some((t) => moduleOfTool(t.name) !== null)).toBe(false);
  // The database tools are documented and shown by their card and the
  // breakdown, not by a row.
  expect(toolShown("db_query")).toBe(true);
  expect(visibleRows().some((r) => r.label === "Database Read Access")).toBe(false);

  localStorage.removeItem("tcm-v2-dev-capture");
  expect(toolShown("save_autorun_script")).toBe(true);
  expect(visibleRows().some((r) => r.label === "Auto Run scripts")).toBe(false);
});

/// Capture mode must never put anything on disk, and no module may change
/// what is registered with real assistants: `loadDisabledTools()` feeds
/// AiBridge's `register` -> `commands.registerAiTool(..., loadDisabledTools(),
/// ...)`. It reads no module state at all, so the same saved list loads the
/// same with capture mode on or off and with any modules on.
test("what loadDisabledTools() returns does not depend on capture mode or on modules", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS, "db_query", "get_tags"]));
  const before = loadDisabledTools();
  expect(before).toEqual(["get_tags"]);

  localStorage.setItem("tcm-v2-dev-capture", "on");
  expect(toolShown("save_autorun_script")).toBe(false);
  expect(loadDisabledTools()).toEqual(before);

  setModulesForTest([]);
  expect(loadDisabledTools()).toEqual(before);

  localStorage.removeItem("tcm-v2-mcp-disabled");
});

/// Switching a module changes no list: the tools of a module are offered
/// while it is on, and the list never names them. The subscription fires for
/// a save, which App re-pushes the bridge context on.
test("switching a module leaves the list as it was, and a save fires the subscription", async () => {
  setModulesForTest([]);
  mockIPC((cmd) => (cmd === "modules_set" ? ["db_access", "auto_run"] : null));
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_tags"]));
  const before = disabledToolsSnapshot();

  const fired = vi.fn();
  const off = subscribeDisabledTools(fired);
  await setModule("auto_run", true);
  expect(visibleRows().map((r) => r.key)).toEqual(["search_test_suites+get_suite_test_cases", "get_run_results", "get_tags", "search_pbis", "search_wiki+get_wiki_page"]);
  expect(disabledToolsSnapshot()).toBe(before);
  expect(before).toEqual(["get_tags"]);

  saveDisabledTools(["get_tags", "search_pbis"]);
  expect(fired).toHaveBeenCalled();
  expect(disabledToolsSnapshot()).toEqual(["get_tags", "search_pbis"]);

  off();
  localStorage.clear();
});

/// Nothing the screen saves can put a module's tool on the list: it would
/// have no switch to take it off again.
test("saving the list never writes a module's tool, even one passed in or already stored", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify([...AUTORUN_TOOLS]));
  saveDisabledTools(toggleRow(loadDisabledTools(), ["search_wiki", "get_wiki_page"]));
  expect(JSON.parse(localStorage.getItem("tcm-v2-mcp-disabled")!)).toEqual(["search_wiki", "get_wiki_page"]);

  saveDisabledTools(["db_query", "get_tags"]);
  expect(JSON.parse(localStorage.getItem("tcm-v2-mcp-disabled")!)).toEqual(["get_tags"]);

  saveDisabledTools([]);
  expect(localStorage.getItem("tcm-v2-mcp-disabled")).toBeNull();
  localStorage.clear();
});

/// The explore tools ride in their modules, as the Rust side puts them.
test("the explore tools belong to their modules", () => {
  expect(moduleOfTool("send_api_request")).toBe("api_templates");
  expect(moduleOfTool("undo_explore_creates")).toBe("api_templates");
  expect(moduleOfTool("record_explore_change")).toBe("auto_run");
  expect(moduleOfTool("get_explore_changes")).toBe("auto_run");
});

test("the ruling tools belong to auto run", () => {
  expect(moduleOfTool("rule_case_divergence")).toBe("auto_run");
  expect(moduleOfTool("clear_autorun_suspected_defect")).toBe("auto_run");
  const names = MCP_TOOLS.map((t) => t.name);
  expect(names.indexOf("rule_case_divergence")).toBe(names.indexOf("report_case_divergence") + 1);
});
