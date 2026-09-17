/**
 * The "what changed" note an upload hands to testers.
 *
 * A tester's first question about an updated case is "have I already run
 * #154650?", so every line leads with the work item id. After that, what
 * moved: the fields with their old and new values, and each step that was
 * changed, added or removed. New cases are listed by id and title alone -
 * nobody has run them yet.
 *
 * Plain text on purpose: it is pasted into chat, email or a ticket.
 *
 * The step-type repair (an ActionStep that should be a ValidateStep) is not
 * a change anyone tests, so a case whose only change is that is left out.
 */
import type { SubmitItemResult, TestCase } from "../bindings";
import type { CaseDiff, StepDiff } from "./caseDiff";

/** One line of a value, however many it had. */
const oneLine = (v: string) => v.replace(/\s*\n+\s*/g, " / ").trim();
const shown = (v: string) => (oneLine(v) === "" ? "(empty)" : `"${oneLine(v)}"`);

function stepLines(d: StepDiff): string[] {
  const n = d.index + 1;
  if (d.kind === "added" && d.new) {
    const out = [`  - Step ${n} added: ${shown(d.new.action)}`];
    if (d.new.expected.trim()) out.push(`      Expected: ${shown(d.new.expected)}`);
    return out;
  }
  if (d.kind === "removed" && d.old) return [`  - Step ${n} removed: ${shown(d.old.action)}`];
  if (d.kind === "changed" && d.old && d.new) {
    const out = [`  - Step ${n} changed`];
    if (d.old.action !== d.new.action) out.push(`      Action: ${shown(d.old.action)} -> ${shown(d.new.action)}`);
    if (d.old.expected !== d.new.expected) {
      out.push(`      Expected: ${shown(d.old.expected)} -> ${shown(d.new.expected)}`);
    }
    return out;
  }
  return [];
}

/** Whether a diff holds anything a tester would act on. */
export function testerVisible(diff: CaseDiff | null | undefined): boolean {
  return Boolean(diff && (diff.fields.length > 0 || diff.steps.detail.length > 0));
}

export function testerNotes({
  pbiId,
  sent,
  results,
  diffs,
}: {
  pbiId: number;
  /** The list that was submitted; every result index points into it. */
  sent: TestCase[];
  results: SubmitItemResult[];
  /** What each sent case changed, aligned with `sent` - measured against
   * Azure DevOps just before the upload. Null for a new case, or when the
   * server copy could not be read. */
  diffs: (CaseDiff | null)[];
}): string {
  const updated: string[] = [];
  const created: string[] = [];

  for (const r of results) {
    const tc = sent[r.index];
    if (!tc) continue;
    const id = r.id ?? tc.update_id;
    if (r.action === "created" && id != null) {
      created.push(`#${id}  ${tc.title}`);
      continue;
    }
    if (r.action !== "updated" || id == null) continue;
    const diff = diffs[r.index];
    if (!diff) {
      // Written, but what changed could not be measured: still worth a
      // tester's look, and honest about why there is no detail.
      updated.push(`#${id}  ${tc.title}`, "  - Updated (details unavailable)");
      continue;
    }
    if (!testerVisible(diff)) continue;
    updated.push(`#${id}  ${tc.title}`);
    for (const f of diff.fields) updated.push(`  - ${f.name}: ${shown(f.old)} -> ${shown(f.new)}`);
    for (const d of diff.steps.detail) updated.push(...stepLines(d));
  }

  const count = (lines: string[]) => lines.filter((l) => l.startsWith("#")).length;
  const parts = [`Test case changes for PBI #${pbiId}`];
  if (updated.length > 0) parts.push("", `Updated (${count(updated)})`, ...updated);
  if (created.length > 0) parts.push("", `New (${created.length})`, ...created);
  if (updated.length === 0 && created.length === 0) parts.push("", "No test case changes to report.");
  return parts.join("\n");
}

/** Whether an upload produced anything worth copying. */
export function hasTesterNotes(results: SubmitItemResult[], diffs: (CaseDiff | null)[]): boolean {
  return results.some(
    (r) =>
      r.action === "created" ||
      (r.action === "updated" && (diffs[r.index] == null || testerVisible(diffs[r.index]))),
  );
}
