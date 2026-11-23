// The server is asked about the chosen database's login app-wide, not only
// while the AI Bridge tab is open: once per login without a verdict, never
// in a loop, never signed out or with Database Access off.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { useAutoDbCheck } from "../../src/app/useAutoDbCheck";
import { saveSelectedDb } from "../../src/lib/dbServer";
import { resetModulesStore, setModulesForTest } from "../../src/lib/modules";

const db = (id: string, over: Record<string, unknown> = {}) => ({
  id,
  label: id,
  server: `${id}.local`,
  database: "Hr",
  user: "someone",
  has_password: true,
  ...over,
});
const stored = { can_write: true, checked_at: 1, reason: "This login can create, update and delete." };

function setup(opts: { verdicts?: { id: string; check: typeof stored }[]; databases?: unknown[] }) {
  const calls: { cmd: string; args: unknown }[] = [];
  let have = opts.verdicts ?? [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "db_databases") return opts.databases ?? [db("a"), db("b")];
    if (cmd === "db_write_verdicts") return have;
    if (cmd === "db_check_write") {
      const id = (args as { id: string }).id;
      have = [...have, { id, check: stored }];
      return stored;
    }
  });
  return calls;
}

function Host({ signedIn = true }: { signedIn?: boolean }) {
  useAutoDbCheck(signedIn);
  return null;
}

function mount(signedIn = true) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={qc}>
      <Host signedIn={signedIn} />
    </QueryClientProvider>,
  );
  return view;
}

const checks = (calls: { cmd: string; args: unknown }[]) => calls.filter((c) => c.cmd === "db_check_write");

beforeEach(() => {
  localStorage.clear();
  setModulesForTest(["db_access"]);
});
afterEach(() => {
  cleanup();
  clearMocks();
  resetModulesStore();
  localStorage.clear();
});

test("with the AI Bridge tab not mounted, a chosen unchecked database is checked once", async () => {
  saveSelectedDb("a");
  const calls = setup({});
  mount();
  await waitFor(() => expect(checks(calls)).toHaveLength(1));
  expect(checks(calls)[0].args).toEqual({ id: "a" });
  // Let the refreshed verdicts settle: still one.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(checks(calls)).toHaveLength(1);
});

test("switching to another unchecked database checks that one too, once each", async () => {
  saveSelectedDb("a");
  const calls = setup({});
  mount();
  await waitFor(() => expect(checks(calls)).toHaveLength(1));
  act(() => saveSelectedDb("b"));
  await waitFor(() => expect(checks(calls)).toHaveLength(2));
  expect(checks(calls)[1].args).toEqual({ id: "b" });
  act(() => saveSelectedDb("a"));
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(checks(calls)).toHaveLength(2);
});

test("a stored verdict triggers no check", async () => {
  saveSelectedDb("a");
  const calls = setup({ verdicts: [{ id: "a", check: stored }] });
  mount();
  await waitFor(() => expect(calls.some((c) => c.cmd === "db_write_verdicts")).toBe(true));
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(checks(calls)).toHaveLength(0);
});

test("signed out, or with Database Access off, nothing is asked", async () => {
  saveSelectedDb("a");
  const calls = setup({});
  mount(false);
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(calls).toHaveLength(0);
  cleanup();
  setModulesForTest([]);
  mount(true);
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(checks(calls)).toHaveLength(0);
});

test("a database with no saved login or password is not asked about", async () => {
  saveSelectedDb("a");
  const calls = setup({ databases: [db("a", { has_password: false })] });
  mount();
  await waitFor(() => expect(calls.some((c) => c.cmd === "db_write_verdicts")).toBe(true));
  await act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
  expect(checks(calls)).toHaveLength(0);
});
