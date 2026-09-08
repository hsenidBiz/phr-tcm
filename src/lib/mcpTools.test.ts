import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { CORE_TOOLS, HIDDEN_TOOLS, loadDisabledTools, MCP_TOOLS, toggleTool, visibleTools } from "./mcpTools";

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

test("core tools cannot be toggled and hidden tools are not offered", () => {
  for (const name of CORE_TOOLS) expect(toggleTool([], name)).toEqual([]);
  expect(visibleTools().map((t) => t.name)).not.toEqual(expect.arrayContaining([...HIDDEN_TOOLS]));
  expect(visibleTools().map((t) => t.name)).toEqual(expect.arrayContaining([...CORE_TOOLS]));
});

test("a saved list naming a core tool is ignored on load", () => {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_test_cases", "search_wiki"]));
  expect(loadDisabledTools()).toEqual(["search_wiki"]);
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
