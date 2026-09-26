// Demo data mode must answer every command that talks to Azure DevOps.
// One it leaves out goes to the real backend, which has no sign-in in demo
// mode: it comes back Unauthorized and the app raises "Session expired" on
// whichever screen loads it. Thirteen had slipped through that way.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";

const read = (...p: string[]) => readFileSync(resolve(__dirname, ...p), "utf8");

test("every Azure DevOps command is patched in demo data", () => {
  // Generated bindings: `name: (args) => typedError<T, AdoError>(...)`.
  const ado = [...read("..", "bindings.ts").matchAll(/^\s*(\w+):\s*\([^)]*\)\s*=>\s*typedError<[^\n]*?AdoError>/gm)].map(
    (m) => m[1],
  );
  expect(ado.length).toBeGreaterThan(40);
  // Patches are the object passed to Object.assign(commands, { ... }), one
  // command per line at four spaces.
  const patched = new Set([...read("demo.ts").matchAll(/^ {4}(\w+):/gm)].map((m) => m[1]));
  expect(ado.filter((name) => !patched.has(name))).toEqual([]);
});
