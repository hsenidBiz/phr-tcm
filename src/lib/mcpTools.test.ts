import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import { MCP_TOOLS, toggleTool } from "./mcpTools";

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
  const once = toggleTool([], "get_test_cases");
  expect(once).toEqual(["get_test_cases"]);
  expect(toggleTool(once, "get_test_cases")).toEqual([]);
});
