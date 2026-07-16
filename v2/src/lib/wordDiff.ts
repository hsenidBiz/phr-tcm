/** Word-level diff for the review's -/+ step lines (git word-diff style):
 * LCS over whitespace-separated words, so "…page in PMS Module (Step 1…"
 * highlights just "in PMS Module" instead of the whole line. */

export type DiffToken = { text: string; changed: boolean };

function lcsSameFlags(a: string[], b: string[]): [boolean[], boolean[]] {
  const n = a.length;
  const m = b.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const sameA = new Array<boolean>(n).fill(false);
  const sameB = new Array<boolean>(m).fill(false);
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      sameA[i] = true;
      sameB[j] = true;
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      i++;
    } else {
      j++;
    }
  }
  return [sameA, sameB];
}

/** Merge neighbours with the same changed-flag so renders stay light. */
function toTokens(words: string[], same: boolean[]): DiffToken[] {
  const out: DiffToken[] = [];
  for (let i = 0; i < words.length; i++) {
    const changed = !same[i];
    const last = out[out.length - 1];
    if (last && last.changed === changed) last.text += ` ${words[i]}`;
    else out.push({ text: words[i], changed });
  }
  return out;
}

/** Both sides tokenized: `old` marks removed words, `new` marks added. */
export function wordDiff(
  oldText: string,
  newText: string,
): { old: DiffToken[]; new: DiffToken[] } {
  const a = oldText.split(/\s+/).filter(Boolean);
  const b = newText.split(/\s+/).filter(Boolean);
  const [sameA, sameB] = lcsSameFlags(a, b);
  return { old: toTokens(a, sameA), new: toTokens(b, sameB) };
}
