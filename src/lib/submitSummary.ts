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
import type { SubmitItemResult } from "../bindings";
import { plural } from "./plural";

export type SubmitSummary = {
  created: number;
  updated: number;
  failed: number;
  /** May or may not exist in Azure DevOps - held until checked. */
  unknown: number;
  headline: string;
  /** The failed cases grouped by what Azure DevOps said, most common first:
   * "64 failed: Module is required". Empty when nothing failed. */
  failures: string[];
};

export function summariseSubmit(results: SubmitItemResult[]): SubmitSummary {
  const created = results.filter((r) => r.action === "created").length;
  const updated = results.filter((r) => r.action === "updated").length;
  const failed = results.filter((r) => r.action === "failed").length;
  const unknown = results.filter((r) => r.action === "unknown").length;

  const parts: string[] = [];
  if (created > 0) parts.push(`${created} created`);
  if (updated > 0) parts.push(`${updated} updated`);
  if (failed > 0) parts.push(`${failed} failed`);
  if (unknown > 0) parts.push(`${unknown} unknown`);

  // A failed case that came back with no reason is counted under the
  // sentence the row itself would show.
  const byReason = new Map<string, number>();
  for (const r of results) {
    if (r.action !== "failed") continue;
    const why = (r.error ?? "").trim() || "Azure DevOps gave no reason";
    byReason.set(why, (byReason.get(why) ?? 0) + 1);
  }
  const failures = [...byReason.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([why, n]) => `${n} failed: ${why}`);

  const uploaded = created + updated;
  const lead = uploaded === 0 ? "Nothing uploaded" : `${plural(uploaded, "test case")} uploaded`;

  return {
    created,
    updated,
    failed,
    unknown,
    headline: parts.length ? `${lead} - ${parts.join(", ")}` : lead,
    failures,
  };
}
