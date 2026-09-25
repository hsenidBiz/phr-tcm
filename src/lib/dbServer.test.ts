import { afterEach, beforeEach, expect, test, vi } from "vitest";

// The migration's only IPC. Mocked at the module so each test decides what
// Rust answers, and can count how often it was asked.
const importLegacy = vi.fn();
const logUi = vi.fn();
vi.mock("../bindings", () => ({
  commands: {
    importLegacyDbConnection: (cs: string) => importLegacy(cs),
    logUi: (m: string) => {
      logUi(m);
      return Promise.resolve(null);
    },
  },
}));

import {
  dbWritesSnapshot,
  forgetDbConfig,
  isDevLoginConnection,
  isDevLoginUser,
  loadDbConfig,
  loadDbWrites,
  loadSelectedDb,
  migrateLegacyDbConnection,
  saveDbConfig,
  saveDbWrites,
  saveSelectedDb,
  selectedDbSnapshot,
  subscribeDbSettings,
} from "./dbServer";

beforeEach(() => {
  importLegacy.mockReset();
  logUi.mockReset();
});
afterEach(() => localStorage.clear());

const settings = { exe_path: "C:/tools/x.exe", db_type: "mssql", schema_filter: "dbo" };

/// Off is the only default a switch like this may have, and it has to be
/// off for BOTH ways a profile can arrive with nothing stored: never
/// configured, and cleared.
test("creating, updating and deleting is off until it is switched on", () => {
  expect(loadDbWrites()).toBe(false);

  saveDbWrites(true);
  expect(localStorage.getItem("tcm-v2-db-writes")).toBe("1");
  expect(loadDbWrites()).toBe(true);

  saveDbWrites(false);
  // Absent rather than "0": a cleared profile then reads exactly the same
  // as a fresh one.
  expect(localStorage.getItem("tcm-v2-db-writes")).toBeNull();
  expect(loadDbWrites()).toBe(false);
});

test("the selected database and the write switch notify, so App can re-push them", () => {
  const seen = vi.fn();
  const stop = subscribeDbSettings(seen);

  expect(loadSelectedDb()).toBe("");
  saveSelectedDb("dev-read");
  expect(seen).toHaveBeenCalledTimes(1);
  expect(localStorage.getItem("tcm-v2-db-selected")).toBe("dev-read");
  expect(selectedDbSnapshot()).toBe("dev-read");

  saveDbWrites(true);
  expect(seen).toHaveBeenCalledTimes(2);
  expect(dbWritesSnapshot()).toBe(true);

  forgetDbConfig();
  expect(seen).toHaveBeenCalledTimes(3);
  // Nothing chosen reads as the empty string - what App turns into the
  // null the bridge treats as "none".
  expect(selectedDbSnapshot()).toBe("");

  stop();
  saveDbWrites(false);
  expect(seen).toHaveBeenCalledTimes(3);
});

/// The blob holds only the PHR X server's non-secret settings now.
test("the stored settings carry no connection string", () => {
  saveDbConfig(settings);
  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp")!);
  expect(stored).toEqual(settings);
  expect(loadDbConfig()).toEqual(settings);
});

/// A mirror of `db::guard::access_for`, which is the door that enforces
/// it. This copy only decides whether the write switch can be moved.
test("only a user id ending in _devlogin is the dev login", () => {
  expect(isDevLoginConnection("Server=d;Database=a;User Id=sgdev01db01_devlogin;Password=p;")).toBe(
    true,
  );
  // The key is folded the way a driver folds it: spaces out, case ignored.
  expect(isDevLoginConnection("server=d;UID=SGDEV01DB01_DEVLOGIN;")).toBe(true);
  expect(isDevLoginConnection("Server=d;Database=a;User Id=sgdev01db02_readonly;")).toBe(false);
  expect(isDevLoginConnection("Server=d;Database=a;")).toBe(false);
  expect(isDevLoginConnection("")).toBe(false);
  // Never the password, whatever it happens to contain.
  expect(isDevLoginConnection("Server=d;User Id=ro;Password=x_devlogin;")).toBe(false);
});

test("the same rule applied to a user name", () => {
  expect(isDevLoginUser("sgdev01db01_devlogin")).toBe(true);
  expect(isDevLoginUser("  SGDEV01DB01_DEVLOGIN ")).toBe(true);
  expect(isDevLoginUser("sgdev01db02_readonly")).toBe(false);
  expect(isDevLoginUser("")).toBe(false);
});

// ------------------------------------------------------------- migration

const LEGACY = "Server=x;Database=HR;User Id=me;Password=secret;";

test("a stored connection string moves into Rust once, and leaves the blob", async () => {
  localStorage.setItem("tcm-v2-db-mcp", JSON.stringify({ ...settings, connection_string: LEGACY }));
  importLegacy.mockResolvedValue({ status: "ok", data: "own" });

  await migrateLegacyDbConnection();

  expect(importLegacy).toHaveBeenCalledTimes(1);
  expect(importLegacy).toHaveBeenCalledWith(LEGACY);
  expect(loadSelectedDb()).toBe("own");
  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp")!);
  expect(stored).toEqual(settings);
  expect(localStorage.getItem("tcm-v2-db-mcp")).not.toContain("secret");

  // Strictly one-time: nothing left to import, so a login saved later can
  // never be replaced by a leftover.
  await migrateLegacyDbConnection();
  expect(importLegacy).toHaveBeenCalledTimes(1);
});

test("a failed import leaves the blob untouched, for the next start to retry", async () => {
  const blob = JSON.stringify({ ...settings, connection_string: LEGACY });
  localStorage.setItem("tcm-v2-db-mcp", blob);
  importLegacy.mockResolvedValue({
    status: "error",
    error: "Could not save the login in Windows Credential Manager.",
  });

  await migrateLegacyDbConnection();

  expect(localStorage.getItem("tcm-v2-db-mcp")).toBe(blob);
  expect(loadSelectedDb()).toBe("");
  expect(logUi).toHaveBeenCalledTimes(1);
  // The log line carries Rust's sentence, never the string itself.
  expect(logUi.mock.calls[0][0]).not.toContain("secret");
});

test("with no connection string stored there is nothing to import", async () => {
  localStorage.setItem("tcm-v2-db-mcp", JSON.stringify(settings));
  await migrateLegacyDbConnection();
  localStorage.setItem("tcm-v2-db-mcp", JSON.stringify({ ...settings, connection_string: "  " }));
  await migrateLegacyDbConnection();
  localStorage.removeItem("tcm-v2-db-mcp");
  await migrateLegacyDbConnection();
  expect(importLegacy).not.toHaveBeenCalled();
});

/// Until the move has worked, editing the PHR X settings must not drop the
/// string the next start still has to import.
test("saving settings before the move keeps the string for it", () => {
  localStorage.setItem("tcm-v2-db-mcp", JSON.stringify({ ...settings, connection_string: LEGACY }));
  saveDbConfig({ ...settings, exe_path: "D:/other.exe" });
  const stored = JSON.parse(localStorage.getItem("tcm-v2-db-mcp")!);
  expect(stored.exe_path).toBe("D:/other.exe");
  expect(stored.connection_string).toBe(LEGACY);
});
