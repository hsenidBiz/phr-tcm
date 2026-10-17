// Reading the environments: what the first list may do with the Company
// database card's choice, and when it counts as done.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { activeEnvIdSnapshot, setActiveEnvId } from "./activeEnv";
import { loadEnvironments, switchEnvironment } from "./environments";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const DEFAULT = {
  id: "env-00000001", name: "Default", start_url: "", allowed_origins: [],
  db_id: "dev-read", test_environment: false, has_default_password: false,
};

function mock(databases: () => unknown) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "db_databases") return databases();
    if (cmd === "env_list") return { active: DEFAULT.id, environments: [DEFAULT] };
    if (cmd === "env_save") return { active: DEFAULT.id, environments: [{ ...DEFAULT, db_id: "qa-read" }] };
    return null;
  });
  return calls;
}

test("an unreadable database list neither reconciles nor marks the check done", async () => {
  localStorage.setItem("tcm-v2-db-selected", "qa-read");
  const calls = mock(() => {
    throw "databases unavailable";
  });
  await loadEnvironments();
  expect(calls.find((c) => c.cmd === "env_list")!.args).toEqual({ currentDb: null, workingDir: null });
  expect(calls.some((c) => c.cmd === "env_save")).toBe(false);
  expect(localStorage.getItem("tcm-v2-env-db-reconciled")).toBeNull();
});

test("the check runs again once the list can be read, and then reconciles", async () => {
  localStorage.setItem("tcm-v2-db-selected", "qa-read");
  let readable = false;
  const calls = mock(() => {
    if (!readable) throw "databases unavailable";
    return [{ id: "dev-read" }, { id: "qa-read" }];
  });
  await loadEnvironments();
  expect(localStorage.getItem("tcm-v2-env-db-reconciled")).toBeNull();

  readable = true;
  const view = await loadEnvironments();
  expect(calls.filter((c) => c.cmd === "env_save")).toHaveLength(1);
  expect(view.environments[0].db_id).toBe("qa-read");
  expect(localStorage.getItem("tcm-v2-env-db-reconciled")).toBe("1");
});

test("a list that was read and needs nothing marks the check done", async () => {
  localStorage.setItem("tcm-v2-db-selected", "dev-read");
  const calls = mock(() => [{ id: "dev-read" }]);
  await loadEnvironments();
  expect(calls.some((c) => c.cmd === "env_save")).toBe(false);
  expect(localStorage.getItem("tcm-v2-env-db-reconciled")).toBe("1");
});

// The AI Bridge switches belong to the active environment, so the webview
// has to know which one that is the moment a list or a switch says so.

test("loading the environments makes their active one the switches' environment", async () => {
  mock(() => [{ id: "dev-read" }]);
  await loadEnvironments();
  expect(activeEnvIdSnapshot()).toBe(DEFAULT.id);
});

test("a switch tells Rust the working repository and makes the new one the switches' environment", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  const QA = { ...DEFAULT, id: "env-00000002", name: "QA" };
  const calls: { cmd: string; args: unknown }[] = [];
  const allow = [{ tool: "Claude Code", applied: false, note: "could not read the settings file" }];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "db_databases") return [{ id: "dev-read" }];
    if (cmd === "env_set_active") return { view: { active: QA.id, environments: [DEFAULT, QA] }, allow };
    return null;
  });
  setActiveEnvId(DEFAULT.id);
  const switched = await switchEnvironment(QA.id);
  expect(calls.find((c) => c.cmd === "env_set_active")!.args).toEqual({ id: QA.id, workingDir: "D:\\repo" });
  expect(activeEnvIdSnapshot()).toBe(QA.id);
  // What was done to each AI tool's own settings comes back with it.
  expect(switched.allow).toEqual(allow);
});

/// The same scope rule the Run changes without asking switch uses: with
/// AI tools registered machine-wide, no repository is named.
test("in the machine-wide scope a switch and a list name no repository", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  localStorage.setItem("tcm-v2-ai-global-allowed", "on");
  localStorage.setItem("tcm-v2-ai-scope", "global");
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "db_databases") return [{ id: "dev-read" }];
    if (cmd === "env_list") return { active: DEFAULT.id, environments: [DEFAULT] };
    if (cmd === "env_set_active") return { view: { active: DEFAULT.id, environments: [DEFAULT] }, allow: [] };
    return null;
  });
  await loadEnvironments();
  await switchEnvironment(DEFAULT.id);
  expect(calls.find((c) => c.cmd === "env_list")!.args).toMatchObject({ workingDir: null });
  expect(calls.find((c) => c.cmd === "env_set_active")!.args).toEqual({ id: DEFAULT.id, workingDir: null });
});

test("a list names the working repository, for the allow files Rust brings in line once", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  const calls = mock(() => [{ id: "dev-read" }]);
  await loadEnvironments();
  expect(calls.find((c) => c.cmd === "env_list")!.args).toEqual({ currentDb: null, workingDir: "D:\\repo" });
});

test("a refused switch leaves the switches' environment where it was", async () => {
  mockIPC((cmd) => {
    if (cmd === "env_set_active") throw "the environment cannot be switched";
    return null;
  });
  setActiveEnvId(DEFAULT.id);
  await expect(switchEnvironment("env-00000002")).rejects.toThrow("cannot be switched");
  expect(activeEnvIdSnapshot()).toBe(DEFAULT.id);
});
