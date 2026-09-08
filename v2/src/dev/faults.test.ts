/**
 * The fault injector is dev-only, but two things about it can be quietly
 * wrong in a way that wastes a developer's afternoon rather than showing
 * up: the sniff that decides which commands are safe to patch, and the
 * error object's second shape.
 */
import { beforeEach, expect, test } from "vitest";
import { commands } from "../bindings";
import { describeAdoError } from "../lib/ipc";
import {
  applyDevFaults,
  armFault,
  disarmFault,
  faultSnapshot,
  FAULTS,
  NET_UNREACHABLE,
} from "./faults";

// The originals, captured before the patch - the sniff test compares
// against them by reference.
const beforePatch = {
  appLogs: commands.appLogs,
  appLogDir: commands.appLogDir,
  logUi: commands.logUi,
  setAdoRateLevel: commands.setAdoRateLevel,
  listProjects: commands.listProjects,
};

applyDevFaults();

beforeEach(() => disarmFault());

type Failed = { status: "error"; error: unknown };

test("an armed fault answers instead of the real command", async () => {
  armFault("unreachable", "once");
  const r = (await commands.listProjects("myorg")) as unknown as Failed;
  expect(r.status).toBe("error");
  expect(describeAdoError(r.error as never)).toBe(NET_UNREACHABLE);
});

/**
 * Half the generated commands unwrap with `unwrap` (which reads `.kind`)
 * and half with `unwrapStr` (which does `new Error(r.error)`). Nothing at
 * runtime says which, so the injected error has to satisfy both - or the
 * String half of the app simulates its failures as "[object Object]".
 */
test("the injected error reads correctly through BOTH unwrappers", async () => {
  armFault("unreachable", "once");
  const r = (await commands.listProjects("myorg")) as unknown as Failed;
  expect(describeAdoError(r.error as never)).toBe(NET_UNREACHABLE);
  expect(String(r.error)).toBe(NET_UNREACHABLE);
  expect(new Error(r.error as string).message).toBe(NET_UNREACHABLE);
});

test("'once' is spent by one call and says so", async () => {
  armFault("timeout", "once");
  expect(faultSnapshot().armed).not.toBeNull();
  await commands.listProjects("myorg");
  // Checked instead of making a second call: with nothing armed, the next
  // call would reach real IPC, which does not exist under vitest.
  expect(faultSnapshot().armed).toBeNull();
  expect(faultSnapshot().fired).toBe(true);
});

test("'always' keeps failing until it is stopped", async () => {
  armFault("forbidden", "always");
  for (const _ of [1, 2, 3]) {
    expect(((await commands.listProjects("myorg")) as unknown as Failed).status).toBe("error");
  }
  expect(faultSnapshot().armed?.mode).toBe("always");
});

/**
 * The riskiest thing in the module. Only `typedError` commands may be
 * patched: the bindings also hold infallible ones that resolve to raw
 * data, and handing those a `{ status: "error" }` would not simulate a
 * failure - it would feed the caller a value of a type the command cannot
 * return. If esbuild ever renames `typedError`, the sniff matches nothing
 * and the injector silently stops working; this fails first.
 */
test("only Result-shaped commands are patched", () => {
  expect(commands.listProjects).not.toBe(beforePatch.listProjects);
  // Infallible: no Result to put an error into.
  expect(commands.setAdoRateLevel).toBe(beforePatch.setAdoRateLevel);
  // Excluded by name: the raw error goes to the app log, and a fault mode
  // that also broke the log viewer would hide the evidence it produces.
  expect(commands.appLogs).toBe(beforePatch.appLogs);
  expect(commands.appLogDir).toBe(beforePatch.appLogDir);
  expect(commands.logUi).toBe(beforePatch.logUi);
});

test("every offered fault renders to a non-empty message", () => {
  for (const f of FAULTS) {
    const said = describeAdoError(f.error as never);
    expect(said.trim().length, f.id).toBeGreaterThan(0);
    expect(said, f.id).not.toContain("[object Object]");
  }
});
