// The AI Bridge switches belong to an environment: the tools an assistant
// may use, database writes and API template writes are kept per environment,
// read for the active one, and moved over once from the single values older
// versions kept.

import { afterEach, expect, test, vi } from "vitest";
import { activeEnvIdSnapshot, migrateBridgeSwitches, noteEnvironments, setActiveEnvId } from "./activeEnv";
import { apiWritesSnapshot, loadApiWrites, saveApiWrites, subscribeApiWrites } from "./apiTemplates";
import { dbWritesSnapshot, loadDbWrites, saveDbWrites, subscribeDbSettings } from "./dbServer";
import { disabledToolsSnapshot, loadDisabledTools, saveDisabledTools, subscribeDisabledTools } from "./mcpTools";

afterEach(() => localStorage.clear());

const env = (id: string, name: string) => ({
  id,
  name,
  start_url: "",
  allowed_origins: [],
  db_id: "dev-read",
  test_environment: false,
  test_prefix: "AUTOTEST",
  has_default_password: false,
  db_auto_approve: false, password_expiry_query: "", active_session_query: "", reset_expired_passwords: false,
});
const A = env("env-0000000a", "Default");
const B = env("env-0000000b", "QA");
const C = env("env-0000000c", "Staging");
const view = (active: string, ...environments: ReturnType<typeof env>[]) => ({ active, playwright_export: active, environments });

/** What an older version kept: one value for the whole app. */
function legacy() {
  localStorage.setItem("tcm-v2-mcp-disabled", JSON.stringify(["get_tags"]));
  localStorage.setItem("tcm-v2-db-writes", "1");
  localStorage.setItem("tcm-v2-api-writes", "1");
}

/** The tools set a fresh install starts with: what is read with nothing stored. */
function freshTools(): string[] {
  const saved = { ...localStorage };
  localStorage.clear();
  const fresh = loadDisabledTools();
  for (const [k, v] of Object.entries(saved)) localStorage.setItem(k, v);
  return fresh;
}

test("migration gives every existing environment today's tools, and only the active one today's writes", () => {
  legacy();
  noteEnvironments(view(B.id, A, B));

  expect(activeEnvIdSnapshot()).toBe(B.id);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  expect(loadDbWrites()).toBe(true);
  expect(loadApiWrites()).toBe(true);

  setActiveEnvId(A.id);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
  expect(loadDbWrites()).toBe(false);
  expect(loadApiWrites()).toBe(false);

  // The single values are gone, so they can never apply anywhere else.
  expect(localStorage.getItem("tcm-v2-mcp-disabled")).toBeNull();
  expect(localStorage.getItem("tcm-v2-db-writes")).toBeNull();
  expect(localStorage.getItem("tcm-v2-api-writes")).toBeNull();
});

test("migration runs once: a later list neither re-applies nor resurrects anything", () => {
  legacy();
  noteEnvironments(view(A.id, A, B));
  // Something writes the old keys again (an older build, say).
  legacy();
  noteEnvironments(view(B.id, A, B));

  expect(loadDbWrites()).toBe(false);
  expect(loadApiWrites()).toBe(false);
  setActiveEnvId(A.id);
  saveDisabledTools([]);
  migrateBridgeSwitches(view(A.id, A, B));
  expect(loadDisabledTools()).toEqual([]);
});

test("an environment made after the migration starts with the fresh-install tools and every write off", () => {
  legacy();
  noteEnvironments(view(A.id, A, B));
  noteEnvironments(view(C.id, A, B, C));

  expect(loadDisabledTools()).toEqual(freshTools());
  expect(loadDbWrites()).toBe(false);
  expect(loadApiWrites()).toBe(false);
});

test("a list with no active environment migrates nothing", () => {
  legacy();
  noteEnvironments({ active: "", playwright_export: "", environments: [] });
  expect(localStorage.getItem("tcm-v2-db-writes")).toBe("1");
  expect(activeEnvIdSnapshot()).toBe("");
});

test("each environment keeps its own switches", () => {
  noteEnvironments(view(A.id, A, B));
  saveDbWrites(true);
  saveApiWrites(true);
  saveDisabledTools(["get_tags"]);

  setActiveEnvId(B.id);
  expect(loadDbWrites()).toBe(false);
  expect(loadApiWrites()).toBe(false);
  expect(loadDisabledTools()).toEqual([]);
  saveDisabledTools(["get_run_results"]);

  setActiveEnvId(A.id);
  expect(loadDbWrites()).toBe(true);
  expect(loadApiWrites()).toBe(true);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
});

test("Turn all on acts on the active environment only", () => {
  noteEnvironments(view(A.id, A, B));
  saveDisabledTools(["get_tags"]);
  setActiveEnvId(B.id);
  saveDisabledTools(["get_tags"]);

  setActiveEnvId(A.id);
  saveDisabledTools([]);
  expect(loadDisabledTools()).toEqual([]);
  setActiveEnvId(B.id);
  expect(loadDisabledTools()).toEqual(["get_tags"]);
});

test("a switch tells every store's readers, and their snapshots follow it", () => {
  noteEnvironments(view(A.id, A, B));
  saveDbWrites(true);
  saveApiWrites(true);
  saveDisabledTools(["get_tags"]);
  expect(dbWritesSnapshot()).toBe(true);
  expect(apiWritesSnapshot()).toBe(true);
  expect(disabledToolsSnapshot()).toEqual(["get_tags"]);

  const tools = vi.fn();
  const db = vi.fn();
  const api = vi.fn();
  const stops = [subscribeDisabledTools(tools), subscribeDbSettings(db), subscribeApiWrites(api)];
  setActiveEnvId(B.id);
  for (const stop of stops) stop();

  expect(tools).toHaveBeenCalled();
  expect(db).toHaveBeenCalled();
  expect(api).toHaveBeenCalled();
  expect(dbWritesSnapshot()).toBe(false);
  expect(apiWritesSnapshot()).toBe(false);
  expect(disabledToolsSnapshot()).toEqual([]);
});
