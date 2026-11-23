import { QueryClient } from "@tanstack/react-query";
import { beforeEach, expect, test, vi } from "vitest";

const dbCheckWrite = vi.fn();
vi.mock("../../src/bindings", () => ({
  commands: {
    dbCheckWrite: (id: string) => dbCheckWrite(id),
    logUi: () => Promise.resolve(null),
  },
}));

import { CHECK_FAILED, reasonFor, recheckDb, useDbCheckProgress } from "../../src/lib/dbWriteCheck";
import { renderHook } from "@testing-library/react";

beforeEach(() => {
  dbCheckWrite.mockReset();
});

async function failWith(id: string, error: unknown) {
  dbCheckWrite.mockResolvedValue({ status: "error", error });
  await recheckDb(new QueryClient(), id);
  const { result } = renderHook(() => useDbCheckProgress());
  return reasonFor(null, result.current.failed.has(id), result.current.reasons.get(id));
}

test("Rust's own fixed sentences are shown as they are", async () => {
  const notInstalled =
    "sqlcmd is not installed on this machine - install it with `winget install sqlcmd` (or the SQL Server command line tools), then try again";
  expect(await failWith("w1", notInstalled)).toBe(notInstalled);
  const noLogin =
    "no login is saved for this database yet - save one with its Edit button under Database Read Access on the AI Bridge tab";
  expect(await failWith("w2", noLogin)).toBe(noLogin);
});

test("anything else, transport text included, shows the generic sentence", async () => {
  expect(await failWith("w3", "error sending request for url (https://dev.azure.com/x?secret)")).toBe(CHECK_FAILED);
  expect(await failWith("w4", { oops: 1 })).toBe(CHECK_FAILED);
  dbCheckWrite.mockImplementation(() => {
    throw new Error("boom with a host name");
  });
  await recheckDb(new QueryClient(), "w5");
  const { result } = renderHook(() => useDbCheckProgress());
  expect(reasonFor(null, result.current.failed.has("w5"), result.current.reasons.get("w5"))).toBe(CHECK_FAILED);
});

test("a later good check clears the earlier reason", async () => {
  const noLogin = "no login is saved for this database yet - save one";
  expect(await failWith("w6", noLogin)).toBe(noLogin);
  dbCheckWrite.mockResolvedValue({ status: "ok", data: { can_write: true, checked_at: 1, reason: "x" } });
  await recheckDb(new QueryClient(), "w6");
  const { result } = renderHook(() => useDbCheckProgress());
  expect(result.current.failed.has("w6")).toBe(false);
  expect(result.current.reasons.has("w6")).toBe(false);
});
