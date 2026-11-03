// One way to match a search box against a row, shared by every list screen.
// The typed text is split into words (a "quoted phrase" is one word); a row
// matches when EVERY word is found in at least one of its fields. A number
// field is an id, so "#123" finds it as well as "123".
//
// A comma separates alternatives ("1234, 5678"): a row matches when it
// matches ANY of them. A comma inside "quotes" is just a character.

type Field = string | number | null | undefined;

/** The typed text as words: split on whitespace, "quoted phrases" kept whole. */
function words(query: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"?|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(query)) !== null) {
    const w = (m[1] ?? m[2] ?? "").trim().toLowerCase();
    if (w !== "") out.push(w);
  }
  return out;
}

/** The query's comma-separated alternatives, trimmed, empty and repeated
 * ones dropped; a comma inside "quotes" does not split. A search that goes
 * to a server runs one request per entry. */
export function searchAlternatives(query: string): string[] {
  const parts: string[] = [];
  let cur = "";
  let quoted = false;
  for (const ch of query) {
    if (ch === '"') quoted = !quoted;
    if (ch === "," && !quoted) {
      parts.push(cur);
      cur = "";
    } else {
      cur += ch;
    }
  }
  parts.push(cur);
  const seen = new Set<string>();
  const out: string[] = [];
  for (const p of parts) {
    const t = p.trim();
    const key = t.toLowerCase();
    if (t === "" || seen.has(key)) continue;
    seen.add(key);
    out.push(t);
  }
  return out;
}

export function matchesQuery(fields: ReadonlyArray<Field>, query: string): boolean {
  const alts = searchAlternatives(query)
    .map(words)
    .filter((ws) => ws.length > 0);
  if (alts.length === 0) return true;
  const texts: string[] = [];
  for (const f of fields) {
    if (f == null) continue;
    const s = String(f).toLowerCase();
    texts.push(s);
    if (typeof f === "number") texts.push(`#${s}`);
  }
  return alts.some((ws) => ws.every((w) => texts.some((t) => t.includes(w))));
}

/** "Search by name, id, tag, state…": the first four names, then an ellipsis. */
export function searchPlaceholder(names: string[]): string {
  return `Search by ${names.slice(0, 4).join(", ")}${names.length > 4 ? "…" : ""}`;
}

/** A priority as people write it ("P2"), or nothing when there is none. */
export function priorityText(p: number | null | undefined): string | null {
  return p == null ? null : `P${p}`;
}

/** What the Update and View Test Cases lists search in a case. */
export function caseSearchFields(c: {
  id: number;
  title: string;
  tags: string;
  module_value: string;
  automation_status: string;
  priority: number | null;
}): Field[] {
  return [c.id, c.title, c.tags, c.module_value, c.automation_status, priorityText(c.priority)];
}

export const CASE_SEARCH_PLACEHOLDER = searchPlaceholder([
  "name",
  "id",
  "tag",
  "module",
  "automation status",
  "priority",
]);
