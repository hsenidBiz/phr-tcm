/**
 * What a finished submit adds up to, for the panel that reports it.
 *
 * Kept apart from the panel itself because the counting has one rule that
 * is easy to get wrong by eye: a case that was written and THEN hit
 * trouble - the suite link, say - exists in Azure DevOps. It is uploaded,
 * and its own row carries the warning; folding it into the failure count
 * would tell someone to retry a case that is already there, and this app
 * cannot delete the duplicate that would make.
 */
import type { SubmitItemResult, TestCase } from "../bindings";
import { keysFor } from "./fileSync";

export type SubmitSummary = {
  created: number;
  updated: number;
  failed: number;
  headline: string;
};

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function summariseSubmit(results: SubmitItemResult[]): SubmitSummary {
  const created = results.filter((r) => r.action === "created").length;
  const updated = results.filter((r) => r.action === "updated").length;
  const failed = results.filter((r) => r.action === "failed").length;

  const parts: string[] = [];
  if (created > 0) parts.push(`${created} created`);
  if (updated > 0) parts.push(`${updated} updated`);
  if (failed > 0) parts.push(`${failed} failed`);

  const uploaded = created + updated;
  const lead = uploaded === 0 ? "Nothing uploaded" : `${plural(uploaded, "test case")} uploaded`;

  return { created, updated, failed, headline: parts.length ? `${lead} - ${parts.join(", ")}` : lead };
}

/**
 * The keys of the rows that failed, so the queue can ring the ones still
 * sitting there needing a decision.
 *
 * Numbered over the WHOLE sent list, then selected - the same discipline
 * `pruneCreated` documents. Two queued cases can legitimately share a
 * title, and numbering over the selection alone shifts the occurrences so
 * the wrong row gets marked.
 */
export function failedKeys(sent: TestCase[], results: SubmitItemResult[]): Set<string> {
  const keys = keysFor(sent);
  const out = new Set<string>();
  for (const r of results) {
    if (r.action !== "failed") continue;
    const k = keys[r.index];
    if (k != null) out.add(k);
  }
  return out;
}
