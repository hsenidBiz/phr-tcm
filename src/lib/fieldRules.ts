/**
 * What the project's Test Case field rules say about the queue, shaped for
 * the screen. The check itself is Rust's (`check_field_rules`); this only
 * lays its answer out: one warning per row, and a summary that groups the
 * rows with the same problem.
 */
import type { RuleCheck, RuleProblem, TestCase } from "../bindings";
import { plural } from "./plural";

/** The parts of a case the rules can judge, and nothing else: the query key
 * built from this changes when one of them does, not on every edit to a
 * step or a title. */
export function ruleInputs(queue: TestCase[]): string {
  return JSON.stringify(
    queue.map((tc) => [tc.update_id ?? null, tc.module_value, tc.tags, tc.preconditions.trim() === "" ? 0 : 1]),
  );
}

/** One warning per queue row, or null where the row is fine. A row with
 * several problems reads them one after another. */
export function rowFieldProblems(check: RuleCheck | null | undefined, length: number): (string | null)[] {
  const out: (string | null)[] = Array.from({ length }, () => null);
  for (const p of check?.problems ?? []) {
    if (p.index < 0 || p.index >= length) continue;
    out[p.index] = out[p.index] ? `${out[p.index]} ${p.message}` : p.message;
  }
  return out;
}

/** Why the cases cannot be sent yet, one line per field and kind of
 * problem: "3 cases need a Module before they can be sent". */
export function fieldRuleSummary(problems: RuleProblem[]): string[] {
  const groups = new Map<string, { kind: string; field: string; rows: Set<number> }>();
  for (const p of problems) {
    const key = `${p.kind}\u0000${p.reference_name}`;
    const g = groups.get(key) ?? { kind: p.kind, field: p.field, rows: new Set<number>() };
    g.rows.add(p.index);
    groups.set(key, g);
  }
  return [...groups.values()].map((g) =>
    g.kind === "required"
      ? `${plural(g.rows.size, "case")} ${g.rows.size === 1 ? "needs" : "need"} a ${g.field} before ${g.rows.size === 1 ? "it" : "they"} can be sent`
      : `${plural(g.rows.size, "case")} ${g.rows.size === 1 ? "has" : "have"} a ${g.field} that is not one of the allowed values`,
  );
}

/** The sentence shown once when the rules could not be read. */
export const RULES_UNCHECKED =
  "The field rules for Test Cases in this project could not be checked, so the cases are sent as they are. " +
  "Azure DevOps may still refuse a case, and says why on its row.";
