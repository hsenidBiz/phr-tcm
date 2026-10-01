// Reading the environments: what the first list may do with the Company
// database card's choice, and when it counts as done.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import { loadEnvironments } from "./environments";

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
  expect(calls.find((c) => c.cmd === "env_list")!.args).toEqual({ currentDb: null });
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
