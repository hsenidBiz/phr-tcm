import { afterEach, expect, test, vi } from "vitest";
import {
  dbConnectionSnapshot,
  dbWritesSnapshot,
  forgetDbConfig,
  isDevLoginConnection,
  loadDbWrites,
  saveDbConfig,
  saveDbWrites,
  subscribeDbSettings,
} from "./dbServer";

afterEach(() => localStorage.clear());

const config = (connection_string: string) => ({
  exe_path: "",
  db_type: "mssql",
  connection_string,
  schema_filter: "",
});

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

test("the connection and the write switch notify, so App can re-push them", () => {
  const seen = vi.fn();
  const stop = subscribeDbSettings(seen);

  saveDbConfig(config("Server=dev;Database=a;User Id=ro;Password=p;"));
  expect(seen).toHaveBeenCalledTimes(1);
  expect(dbConnectionSnapshot()).toBe("Server=dev;Database=a;User Id=ro;Password=p;");

  saveDbWrites(true);
  expect(seen).toHaveBeenCalledTimes(2);
  expect(dbWritesSnapshot()).toBe(true);

  forgetDbConfig();
  expect(seen).toHaveBeenCalledTimes(3);
  // No connection chosen reads as the empty string - what App turns into
  // the null the bridge treats as "none".
  expect(dbConnectionSnapshot()).toBe("");

  stop();
  saveDbWrites(false);
  expect(seen).toHaveBeenCalledTimes(3);
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
