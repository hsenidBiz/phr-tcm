/**
 * After a submit, the JSON file a case came from is STALE: it still says
 * "create me" for a case that now exists in Azure DevOps. Re-importing it
 * would create the whole set again - the exact duplicate trap this app is
 * built to avoid.
 *
 * This computes each owning file's post-submit contents: every case that
 * succeeded carries its work item id (created cases gain one, updates keep
 * theirs) and exactly the content that was uploaded - so re-importing the
 * file yields updates the review gate marks "no-op - nothing will change"
 * and the submit skips. Failed cases stay as they were, still ready to
 * retry. App-only fields ride along untouched: the per-case `comment` and
 * `reviewer_notes` live ON the case, and the file-level `comments` block
 * is preserved by the write itself (merge_cases_into_draft keeps every
 * top-level key it does not own).
 */

import type { TestCase } from "../bindings";

export type StampOutcome = {
  index: number;
  action: string; // "created" | "updated" | "failed" | "skipped"
  id?: number | null;
};

export type StampedFile = { slice: TestCase[]; changed: boolean };

/**
 * Per owning file: the case list to write back, in queue order.
 *
 * `prev` is the queue AS SUBMITTED, `owners` its owner path per index
 * (computed pre-submit - ownership matching is title-derived and a submit
 * does not rename, but the convention from bulk edits is kept). `sent` is
 * the filtered list every result index points into; its elements are the
 * same objects as `prev`'s, so identity lookup maps them back.
 */
export function stampFileSlices(
  prev: TestCase[],
  owners: string[],
  sent: TestCase[],
  results: StampOutcome[],
): Map<string, StampedFile> {
  // Post-submit form per queue index, only for cases that succeeded.
  const post = new Map<number, TestCase>();
  for (const r of results) {
    const sentCase = sent[r.index];
    if (!sentCase) continue;
    const qi = prev.indexOf(sentCase);
    if (qi < 0) continue;
    if (r.action === "created" && r.id != null) {
      post.set(qi, { ...sentCase, update_id: r.id });
    } else if (r.action === "updated") {
      post.set(qi, sentCase);
    }
    // "failed": the file keeps the case exactly as it was - still a draft,
    // still ready to retry.
  }

  const files = new Map<string, StampedFile>();
  prev.forEach((tc, i) => {
    const path = owners[i];
    if (!path) return;
    const f = files.get(path) ?? { slice: [], changed: false };
    f.slice.push(post.get(i) ?? tc);
    if (post.has(i)) f.changed = true;
    files.set(path, f);
  });
  return files;
}

/** Titles of successfully CREATED cases whose new work item id could not
 * be written into any file - no owning watch matched them (a rename the
 * file never learned about), or the queue never had a file at all (a
 * shared-draft or hand-typed queue). Every one of these is a future
 * duplicate: the id lives only in Azure DevOps now, and importing the
 * same draft again will create the case a second time. The caller says
 * so, loudly - both real incidents (43 and 3 duplicates) happened
 * because this situation was silent. */
export function unstampedCreated(
  prev: TestCase[],
  owners: string[],
  sent: TestCase[],
  results: StampOutcome[],
): string[] {
  const out: string[] = [];
  for (const r of results) {
    if (r.action !== "created" || r.id == null) continue;
    const sentCase = sent[r.index];
    if (!sentCase) continue;
    const qi = prev.indexOf(sentCase);
    if (qi < 0 || !owners[qi]) out.push(sentCase.title);
  }
  return out;
}

/** The (id, comment) pairs a finished submit should copy into the View
 * Test Cases notes store - the same comment, now visible on the case
 * where it lives in Azure DevOps. Created cases use their NEW id. */
export function noteSyncPairs(
  sent: TestCase[],
  results: StampOutcome[],
): Array<{ id: number; comment: string }> {
  const out: Array<{ id: number; comment: string }> = [];
  for (const r of results) {
    const c = sent[r.index];
    const comment = c?.comment ?? "";
    if (!c || !comment.trim()) continue;
    const id = r.action === "created" ? r.id : c.update_id;
    if (id == null) continue;
    if (r.action === "created" || r.action === "updated") {
      out.push({ id, comment });
    }
  }
  return out;
}
