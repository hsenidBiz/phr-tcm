/**
 * The webview half of backup export/import. Everything the app stores in
 * localStorage lives under one prefix, so a backup carries the whole
 * namespace - no per-key registry to fall out of date when a screen adds
 * a setting.
 */

const PREFIX = "tcm-v2-";

/** Every tcm-v2-* key, for the export command. */
export function collectLocalStorage(): { [key: string]: string } {
  const out: { [key: string]: string } = {};
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i);
    if (k && k.startsWith(PREFIX)) out[k] = localStorage.getItem(k) ?? "";
  }
  return out;
}

/**
 * Replace the whole tcm-v2-* namespace with the backup's copy. Keys the
 * backup doesn't have are removed too - an import means "make this machine
 * look like that one", not a merge of both machines' leftovers. Keys
 * outside the prefix are ignored, so a hand-edited backup can't plant
 * anything else in storage. Returns how many keys were applied.
 */
export function applyLocalStorage(entries: { [key: string]: string }): number {
  const stale: string[] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i);
    if (k && k.startsWith(PREFIX) && !(k in entries)) stale.push(k);
  }
  for (const k of stale) localStorage.removeItem(k);
  let applied = 0;
  for (const [k, v] of Object.entries(entries)) {
    if (!k.startsWith(PREFIX)) continue;
    localStorage.setItem(k, v);
    applied++;
  }
  return applied;
}
