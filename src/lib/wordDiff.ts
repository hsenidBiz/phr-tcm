/** Word-level diff for the review's -/+ step lines (git word-diff style):
 * LCS over whitespace-separated words, so "…page in PMS Module (Step 1…"
 * highlights just "in PMS Module" instead of the whole line. */

export type InlineSegment = { text: string; kind: "same" | "added" | "removed" };

/** One merged stream (git's inline word-diff): unchanged words flow as
 * normal text, deleted words appear where they were, inserted words where
 * they land - so a small edit reads as one sentence with marks. */
export function inlineWordDiff(oldText: string, newText: string): InlineSegment[] {
  const a = oldText.split(/\s+/).filter(Boolean);
  const b = newText.split(/\s+/).filter(Boolean);
  const n = a.length;
  const m = b.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: InlineSegment[] = [];
  const push = (text: string, kind: InlineSegment["kind"]) => {
    const last = out[out.length - 1];
    if (last && last.kind === kind) last.text += ` ${text}`;
    else out.push({ text, kind });
  };
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      push(a[i], "same");
      i++;
      j++;
    } else if (j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j])) {
      push(b[j], "added");
      j++;
    } else {
      push(a[i], "removed");
      i++;
    }
  }
  // The split discards whitespace, so an edit that is only line breaks or
  // spacing comes back with every segment marked "same" - the review would
  // show an unmarked line for a value it is about to overwrite. Fall back to
  // the whole value before/after so something always reads as changed.
  if (oldText !== newText && !out.some((s) => s.kind !== "same")) {
    return [
      { text: oldText, kind: "removed" },
      { text: newText, kind: "added" },
    ];
  }
  return out;
}
