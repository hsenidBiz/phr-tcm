/** Smart grouping of test cases by title patterns. Started as a port of v1
 * app/utils/grouping.py and keeps its golden tests; the bracket-tag pass
 * and the fall-throughs were added for titles v1 never saw.
 *
 * 1. Bracket tags: a title that starts with "[Floor Plan][Navigation]"
 *    groups under exactly those tags. A tag set only one case uses joins a
 *    group for its FIRST tag ("[Floor Plan]") when others share it.
 * 2. Delimiter prefix: text before the first category separator
 *    (" - ", ":", "|", "/", ...) is the group key - a short one first. A
 *    longer one ("Individual Detailed Evaluation Report - ...") gets a
 *    second pass of its own, and groups only when other titles share it
 *    word for word: that is a report or screen name, where one title's
 *    separator further in is sentence text ("Verify Format > Bring to
 *    front"), and a group of one falls through.
 * 3. Common word prefix: what is left buckets by first word; any bucket of
 *    >= 2 becomes a folder named after the longest shared run of leading
 *    words. A "][" counts as a word break, so a name never ends mid-tag.
 *
 * Whatever a pass cannot group falls through to the next one rather than
 * straight to Ungrouped - that shortcut is what once put cases in Ungrouped
 * because a " > " in their sentence made a group of one.
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

/** A category is a word or three. Anything longer before the separator is
 * more often the start of a sentence that happens to contain one. */
const MAX_PREFIX_WORDS = 3;

/** ...but a report or screen name runs longer ("Goal Alignment and
 * Cascading Report"), and when several titles share one it is the
 * category. Before the long-prefix pass these went to first-word
 * matching, which put every report starting "Individual" into one group
 * named "Individual". */
const MAX_LONG_PREFIX_WORDS = 8;

/** Separators a word-matched name must not end on: "Report -" is
 * "Report". */
const TRAILING_SEPARATOR = /(\s+(?:-|–|—|:|\||>|>>|\/))+$/;

/** The leading "[A][B]" tags, trimmed and space-collapsed; null when the
 * title does not start with one. */
function leadingTags(title: string): string[] | null {
  const run = /^\s*((?:\[[^\[\]]+\]\s*)+)/.exec(title);
  if (!run) return null;
  const tags = [...run[1].matchAll(/\[([^\[\]]+)\]/g)]
    .map((m) => m[1].trim().replace(/\s+/g, " "))
    .filter(Boolean);
  return tags.length ? tags : null;
}

const tagName = (tags: string[]) => tags.map((t) => `[${t}]`).join("");

function delimiterPrefix(title: string, maxWords: number): string | null {
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
  if (!bestPrefix || words(bestPrefix).length > maxWords) return null;
  return bestPrefix;
}

function words(s: string): string[] {
  const t = s.trim().replace(/\]\[/g, "] [");
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
  const name = common.length ? common.join(" ") : first[0];
  return name.replace(TRAILING_SEPARATOR, "") || name;
}

export function groupIndices(titles: string[]): TitleGroup[] {
  const groups: TitleGroup[] = [];
  const clean = titles.map((raw) => (raw ?? "").trim());

  /** Bucket `idx` by `keyOf`; buckets of >= 2 become groups, and the rest
   * are returned for the next pass. */
  const pass = (
    idx: number[],
    keyOf: (i: number) => { key: string; name: string } | null,
  ): number[] => {
    const buckets = new Map<string, { name: string; idx: number[] }>();
    const left: number[] = [];
    for (const i of idx) {
      const k = keyOf(i);
      if (!k) {
        left.push(i);
        continue;
      }
      const b = buckets.get(k.key) ?? { name: k.name, idx: [] };
      b.idx.push(i);
      buckets.set(k.key, b);
    }
    for (const b of buckets.values()) {
      if (b.idx.length >= 2) groups.push({ name: b.name, indices: b.idx });
      else left.push(...b.idx);
    }
    return left;
  };

  const all = clean.map((_, i) => i);
  const tagsOf = clean.map((t) => (t ? leadingTags(t) : null));

  // Pass 1: the full tag run, then the first tag for the leftovers.
  let left = pass(all, (i) => {
    const tags = tagsOf[i];
    return tags ? { key: tags.join("|").toLowerCase(), name: tagName(tags) } : null;
  });
  left = pass(left, (i) => {
    const tags = tagsOf[i];
    return tags ? { key: tags[0].toLowerCase(), name: tagName([tags[0]]) } : null;
  });

  // Pass 2: a short category before a separator, then a longer one that
  // other titles share.
  for (const maxWords of [MAX_PREFIX_WORDS, MAX_LONG_PREFIX_WORDS]) {
    left = pass(left, (i) => {
      const prefix = clean[i] ? delimiterPrefix(clean[i], maxWords) : null;
      return prefix ? { key: prefix.toLowerCase(), name: prefix } : null;
    });
  }

  // Pass 3: common-word-prefix clustering.
  const ungrouped: number[] = [];
  const wordBuckets = new Map<string, number[]>();
  for (const i of left) {
    const w = words(clean[i]);
    const firstWord = w.length ? w[0].toLowerCase() : "";
    wordBuckets.set(firstWord, [...(wordBuckets.get(firstWord) ?? []), i]);
  }
  for (const [firstWord, idxs] of wordBuckets) {
    if (firstWord && idxs.length >= 2) {
      groups.push({ name: commonWordPrefix(idxs.map((i) => clean[i])), indices: idxs });
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
