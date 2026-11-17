import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { CaseScript } from "../../../src/bindings";
import { doneByAutoRun } from "../../../src/screens/AutoRun/byAutoRun";

const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "../../../src-tauri/tests/fixtures/by_auto_run.json");
const table = JSON.parse(readFileSync(FIXTURE, "utf-8")) as { name: string; script: CaseScript; steps: number[] }[];

describe("doneByAutoRun", () => {
  it("counts as the shared fixture says", () => {
    expect(table.length).toBeGreaterThanOrEqual(5);
    for (const row of table) {
      expect([row.name, [...doneByAutoRun(row.script)].sort((a, b) => a - b)]).toEqual([row.name, row.steps]);
    }
  });
});
