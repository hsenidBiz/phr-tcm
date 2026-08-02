import type { TestCase } from "../bindings";

/** Client-side mirror of the Rust TestCase::is_valid rules, so the review
 * gate can flag problems before anything is submitted. The Rust check still
 * runs server-side on submit - this is preview only. */
export function validateCase(tc: TestCase): string | null {
  const title = tc.title.trim();
  if (!title) return "Title is required.";
  if (title.length > 255) return `Title is ${title.length} characters - max 255.`;
  if (tc.steps.length === 0) return "At least one step is required.";
  const bad = tc.steps.findIndex((s) => !s.action.trim());
  if (bad >= 0) return `Step ${bad + 1} action is empty.`;
  if (!["Not Automated", "Planned"].includes(tc.automation_status))
    return `Invalid automation status: '${tc.automation_status}'`;
  if (tc.tags.includes(","))
    return "Tags must be separated with semicolons - commas are not allowed.";
  return null;
}

/** What the duplicate check needs to know about a case already on the PBI. */
export type ExistingCase = { id: number; title: string };

/** Similarity at or above this reads as "the same case in different words".
 * Deliberately high: the one-word-apart siblings that are LEGITIMATELY
 * different cases - "Approve overtime request" / "Reject overtime request",
 * "Export payslip as PDF" / "as CSV" - score 0.67 and must stay silent. A
 * warning that fires on those trains people to ignore it. */
const NEAR_DUPLICATE = 0.8;

/** Words that carry no identity in a test-case title. Removing them lets
 * "Manager can approve overtime" match "Approve overtime as manager" - but
 * only when enough tokens survive; see titleTokens. */
const FILLER = new Set([
  "a", "an", "the", "as", "is", "are", "be", "to", "of", "in", "on", "for",
  "with", "via", "by", "from", "and", "or", "that", "then", "when", "it",
  "can", "should",
]);

function titleTokens(title: string): Set<string> {
  const words = title.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  const meaningful = words.filter((w) => !FILLER.has(w));
  // Filler removal sharpens matching, but on a three-word title it can
  // hollow the set out to almost nothing - and two tiny sets agree far too
  // easily. Short titles are compared whole.
  return new Set(meaningful.length >= 3 ? meaningful : words);
}

/** Dice coefficient over title word sets, 0..1. Order-insensitive, so a
 * reworded title still matches; word-based, so it stays explainable. */
export function titleSimilarity(a: string, b: string): number {
  const ta = titleTokens(a);
  const tb = titleTokens(b);
  if (ta.size === 0 || tb.size === 0) return 0;
  let shared = 0;
  for (const w of ta) if (tb.has(w)) shared++;
  return (2 * shared) / (ta.size + tb.size);
}

/** Duplicate warning (v1 rule: warn only, NEVER a block and never an
 * implicit update). Exact title match keeps its original wording; a
 * near-match names the case it resembles and the score, so the reviewer
 * can decide - the check cannot, and a false positive that blocked a
 * legitimate case would be worse than a missed duplicate. */
export function duplicateWarning(tc: TestCase, existing: ExistingCase[]): string | null {
  if (tc.update_id != null) return null; // explicit update, not a duplicate
  const t = tc.title.trim().toLowerCase();
  if (existing.some((e) => e.title.trim().toLowerCase() === t)) {
    return "A test case with this title already exists on the PBI - this will create a duplicate, not update it.";
  }
  let best: { e: ExistingCase; score: number } | null = null;
  for (const e of existing) {
    const score = titleSimilarity(tc.title, e.title);
    if (score >= NEAR_DUPLICATE && (best == null || score > best.score)) {
      best = { e, score };
    }
  }
  if (!best) return null;
  return (
    `Looks a lot like #${best.e.id} "${best.e.title}" (${Math.round(best.score * 100)}% similar). ` +
    `If it should update that case, set its TestCaseID - otherwise this will create a near-duplicate.`
  );
}
