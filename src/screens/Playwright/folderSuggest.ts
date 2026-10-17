// Folder suggestion for Export to Playwright: a case with no folder of its
// own goes to its area's folder, but the clone often has a folder named for
// what the case is about (a sibling of the area's, or one inside it). When a
// case's title names exactly one such folder, the dialog offers it.

/** Lower-case words of a name or title: split on anything that is not a
 * letter or digit, and on camelCase boundaries (`ratingMethods`, `DWPreview`). */
function words(text: string): string[] {
  return text
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
}

const trimSlashes = (p: string) => p.trim().replace(/[\s/]+$/, "");
const parentOf = (p: string) => p.split("/").slice(0, -1).join("/");
const lastOf = (p: string) => p.split("/").pop() ?? "";

/**
 * The clone's folder a case's title points at, or null. Candidates are the
 * clone's feature paths inside the area's folder or beside it (same parent),
 * never the area's own. A candidate matches when every word of its last
 * folder name - less the words all candidates share, and one-letter words -
 * is a word of the title (singular or plural). The candidate with the most
 * words wins; a tie is ambiguous and suggests nothing.
 */
export function suggestFolder(title: string, areaFolder: string, featurePaths: string[]): string | null {
  const area = trimSlashes(areaFolder);
  if (!area) return null;
  const candidates = [...new Set(featurePaths.map(trimSlashes))].filter(
    (p) => p !== "" && p !== area && (p.startsWith(`${area}/`) || parentOf(p) === parentOf(area)),
  );
  if (candidates.length === 0) return null;

  const names = candidates.map((p) => [...new Set(words(lastOf(p)))]);
  // A prefix every folder carries (`dw`) says nothing about which one is meant.
  // With a single candidate there is nothing to compare, so all its words count.
  const shared = candidates.length > 1 ? names[0].filter((w) => names.every((n) => n.includes(w))) : [];
  const inTitle = new Set(words(title));
  const has = (w: string) => inTitle.has(w) || inTitle.has(`${w}s`) || (w.endsWith("s") && inTitle.has(w.slice(0, -1)));

  let best: string | null = null;
  let bestScore = 0;
  let tied = false;
  candidates.forEach((path, i) => {
    const own = names[i].filter((w) => w.length > 1 && !shared.includes(w));
    if (own.length === 0 || !own.every(has)) return;
    if (own.length > bestScore) {
      best = path;
      bestScore = own.length;
      tied = false;
    } else if (own.length === bestScore) {
      tied = true;
    }
  });
  return tied ? null : best;
}
