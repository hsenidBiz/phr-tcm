import { fileName, type WatchedFile } from "./fileSync";

/** One watched draft file's tester order, offered as a "Start from" choice
 * in Suite Management's suggested run order editor. */
export type TesterOrderSource = {
  path: string;
  label: string;
  /** This suite's cases the file names, in the file's tester order. */
  ids: number[];
  /** Case id -> the case's `area` from the file, the group an upload would
   * have written for it. Cases with no area are absent. */
  groups: Map<number, string>;
};

/**
 * The tester orders the app can offer for cases that are already uploaded.
 *
 * An upload saves a suggested run order only when it creates a case, so an
 * optimized file whose cases were all uploaded earlier never reaches Run
 * Tests on its own. The app still has what it last read from each watched
 * file, and after an upload those cases carry their work item ids - enough
 * to put this suite's cases in the file's tester order.
 *
 * The rules match the upload's: a file is used only when every case in it
 * has a tester order (a half-optimized file is not a tester order), and a
 * tester order only means something inside one file, so each qualifying
 * file is its own source rather than being merged with the others.
 */
export function testerOrderSources(
  watches: readonly WatchedFile[],
  suiteIds: readonly number[],
): TesterOrderSource[] {
  const inSuite = new Set(suiteIds);
  const found: Omit<TesterOrderSource, "label">[] = [];
  for (const w of watches) {
    const rows = w.snapshot;
    if (rows.length === 0 || rows.some((c) => c.tester_order == null)) continue;
    // Stable, so cases sharing a tester order keep the file's own order.
    const ranked = rows
      .map((c, at) => ({ c, at }))
      .filter(({ c }) => c.update_id != null && inSuite.has(c.update_id))
      .sort((a, b) => (a.c.tester_order ?? 0) - (b.c.tester_order ?? 0) || a.at - b.at);
    if (ranked.length === 0) continue;
    const ids: number[] = [];
    const groups = new Map<number, string>();
    for (const { c } of ranked) {
      const id = c.update_id as number;
      if (ids.includes(id)) continue;
      ids.push(id);
      const area = (c.area ?? "").trim();
      if (area) groups.set(id, area);
    }
    found.push({ path: w.path, ids, groups });
  }
  // A file name is the friendly label; two watched files with the same name
  // in different folders are told apart by their full paths.
  const names = found.map((s) => fileName(s.path));
  return found.map((s, i) => ({
    ...s,
    label: `Tester order from ${names.filter((n) => n === names[i]).length > 1 ? s.path : names[i]}`,
  }));
}
