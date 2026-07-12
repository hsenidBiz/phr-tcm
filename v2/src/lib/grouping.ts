/** Smart grouping of test cases by title patterns - port of v1
 * app/utils/grouping.py (same two-pass strategy, same golden tests).
 *
 * 1. Delimiter prefix: text before the first category separator
 *    (" - ", ":", "|", "/", ...) is the group key.
 * 2. Common word prefix: delimiter-free titles bucket by first word; any
 *    bucket of >= 2 becomes a folder named after the longest shared run
 *    of leading words.
 *
 * A folder always has >= 2 members; everything else lands in a final
 * "" (Ungrouped) group. Every index appears exactly once. */

export type TitleGroup = { name: string; indices: number[] };

// A bare "-" is deliberately excluded - too common inside ordinary words
// ("sign-in", "e-mail") to be a safe split.
const DELIMITERS = [
  " - ",
  " – ",
  " — ",
  ": ",
  " : ",
  " | ",
  " > ",
  " >> ",
  " / ",
  "/",
  ":",
  "|",
];

function delimiterPrefix(title: string): string | null {
  let bestI: number | null = null;
  let bestPrefix: string | null = null;
  for (const d of DELIMITERS) {
    const i = title.indexOf(d);
    if (i > 0 && title.slice(i + d.length).trim()) {
      if (bestI === null || i < bestI) {
        bestI = i;
        bestPrefix = title.slice(0, i).trim();
      }
    }
  }
  return bestPrefix || null;
}

function words(s: string): string[] {
  const t = s.trim();
  return t ? t.split(/\s+/) : [];
}

/** Longest run of shared leading words (case-insensitive), displayed in
 * the first title's casing. Always at least one word. */
function commonWordPrefix(titles: string[]): string {
  const split = titles.map(words);
  const first = split[0];
  const common: string[] = [];
  for (let pos = 0; pos < first.length; pos++) {
    const w = first[pos];
    if (split.every((ws) => pos < ws.length && ws[pos].toLowerCase() === w.toLowerCase())) {
      common.push(w);
    } else {
      break;
    }
  }
  return common.length ? common.join(" ") : first[0];
}

export function groupIndices(titles: string[]): TitleGroup[] {
  const delim = new Map<number, { key: string; display: string }>();
  const noDelim: number[] = [];
  titles.forEach((raw, i) => {
    const t = (raw ?? "").trim();
    const prefix = t ? delimiterPrefix(t) : null;
    if (prefix) delim.set(i, { key: prefix.toLowerCase(), display: prefix });
    else noDelim.push(i);
  });

  const groups: TitleGroup[] = [];
  const ungrouped: number[] = [];

  // Pass 1: delimiter-prefix buckets.
  const buckets = new Map<string, { name: string; idx: number[] }>();
  for (const [i, { key, display }] of delim) {
    const b = buckets.get(key) ?? { name: display, idx: [] };
    b.idx.push(i);
    buckets.set(key, b);
  }
  for (const b of buckets.values()) {
    if (b.idx.length >= 2) groups.push({ name: b.name, indices: b.idx });
    else ungrouped.push(...b.idx);
  }

  // Pass 2: common-word-prefix clustering for the delimiter-free titles.
  const wordBuckets = new Map<string, number[]>();
  for (const i of noDelim) {
    const w = words(titles[i]);
    const firstWord = w.length ? w[0].toLowerCase() : "";
    wordBuckets.set(firstWord, [...(wordBuckets.get(firstWord) ?? []), i]);
  }
  for (const [firstWord, idxs] of wordBuckets) {
    if (firstWord && idxs.length >= 2) {
      groups.push({ name: commonWordPrefix(idxs.map((i) => titles[i].trim())), indices: idxs });
    } else {
      ungrouped.push(...idxs);
    }
  }

  groups.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  const result: TitleGroup[] = groups.map((g) => ({
    name: g.name,
    indices: [...g.indices].sort((a, b) => a - b),
  }));
  if (ungrouped.length) result.push({ name: "", indices: [...ungrouped].sort((a, b) => a - b) });
  return result;
}
