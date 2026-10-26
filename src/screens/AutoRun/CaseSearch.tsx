// The search box for the Auto Run case list, at the top of the Setup panel.
// It narrows the list by the case id (with or without its "#") or by words
// in the title, together with the result filters. Several searches can go
// in at once, separated by commas: a case shows when it matches any of
// them. Escape empties it while it has focus; so does the clear button
// beside it.

import { SearchInput } from "../../components/ui/search-input";

type Searchable = { id: number; title: string };

/** One search on its own: "#12" finds the same ids "12" does, every id
 * with 12 in it, and any other text is looked for in the title. */
function matchesOne(c: Searchable, q: string): boolean {
  const bare = q.startsWith("#") ? q.slice(1).trim() : q;
  if (bare !== "" && String(c.id).includes(bare)) return true;
  return c.title.toLowerCase().includes(q);
}

/** Whether a case matches what was typed. Blank text matches every case,
 * and letter case is ignored.
 *
 * Text with commas is several searches, and a case matches when it matches
 * any of them. In such a list a number (with or without "#") means that
 * exact case, so "135592, 135601" shows those two cases and not 1355920;
 * other words still match titles. Empty parts ("12,,13", a trailing comma)
 * are skipped. */
export function matchesSearch(c: Searchable, text: string): boolean {
  const q = text.trim().toLowerCase();
  if (q === "") return true;
  if (!q.includes(",")) return matchesOne(c, q);
  const parts = q
    .split(",")
    .map((p) => p.trim())
    .filter((p) => p !== "" && p !== "#");
  if (parts.length === 0) return true;
  return parts.some((p) => {
    const bare = p.startsWith("#") ? p.slice(1).trim() : p;
    if (/^\d+$/.test(bare)) return c.id === Number(bare);
    return matchesOne(c, p);
  });
}

export default function CaseSearch({
  value,
  onChange,
  wide = false,
}: {
  value: string;
  onChange: (text: string) => void;
  /** The box takes its container's whole width (the Setup panel's). */
  wide?: boolean;
}) {
  return (
    <SearchInput
      aria-label="Search test cases"
      className={wide ? "w-full" : "w-56"}
      placeholder="Search test cases by id or title"
      title="Search the Test cases list by case id or title. Separate several with commas, for example 135592, 135601"
      value={value}
      onChange={onChange}
      onKeyDown={(e) => {
        if (e.key === "Escape" && value !== "") {
          // Only this box's own text: a dialog behind it is not closed.
          e.stopPropagation();
          onChange("");
        }
      }}
    />
  );
}
