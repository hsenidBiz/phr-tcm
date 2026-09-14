import { commands } from "../../bindings";
import { unwrap } from "../../lib/ipc";
import type { SuiteCase } from "../../lib/suiteOrder";

export const suiteCasesKey = (org: string, project: string, planId: number, suiteId: number) =>
  ["suite-cases", org, project, planId, suiteId] as const;

export const plansSuitesKey = (org: string, project: string) => ["plans-suites", org, project] as const;

/** The suite's cases in Azure DevOps' own order. The entries carry the
 * order and the points carry the names; a case with several
 * configurations has several points and one row. */
export async function loadSuiteCases(
  org: string,
  project: string,
  planId: number,
  suiteId: number,
): Promise<SuiteCase[]> {
  const [entries, points] = await Promise.all([
    unwrap(commands.listSuiteEntries(org, project, suiteId)),
    unwrap(commands.listTestPoints(org, project, planId, suiteId)),
  ]);
  const names = new Map<number, string>();
  for (const p of points) {
    if (p.test_case_id != null && !names.has(p.test_case_id)) names.set(p.test_case_id, p.test_case_name);
  }
  return entries
    .filter((e) => e.entry_type === "testCase")
    .map((e) => ({ id: e.id, title: names.get(e.id) ?? `Test case ${e.id}` }));
}
