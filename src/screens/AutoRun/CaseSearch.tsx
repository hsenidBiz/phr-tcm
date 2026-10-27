// The search box for the Auto Run case list, at the top of the Setup panel.
// It narrows the list by the case id (with or without its "#") or by words
// in the title, together with the result filters. Several searches can go
// in at once, separated by commas: a case shows when it matches any of
// them. Escape empties it while it has focus; so does the clear button
// beside it.

import { SearchInput } from "../../components/ui/search-input";
import { matchesQuery, searchPlaceholder } from "../../lib/search";

type Searchable = {
  id: number;
  title: string;
  tags?: string;
  module_value?: string;
  automation_status?: string;
};

/** One search on its own, as several words that must all be found: "#12"
 * finds the same ids "12" does (every id with 12 in it), and any other
 * word is looked for in the title, tags, module, automation status and
 * whatever else the caller passes (its script's area, for one). */
function matchesOne(c: Searchable, q: string, extra: ReadonlyArray<string | null | undefined>): boolean {
  return matchesQuery([c.id, c.title, c.tags, c.module_value, c.automation_status, ...extra], q);
}

/** Whether a case matches what was typed. Blank text matches every case,
 * and letter case is ignored.
 *
 * Text with commas is several searches, and a case matches when it matches
 * any of them. In such a list a number (with or without "#") means that
 * exact case, so "135592, 135601" shows those two cases and not 1355920;
 * other words still match titles. Empty parts ("12,,13", a trailing comma)
 * are skipped. */
export function matchesSearch(
  c: Searchable,
  text: string,
  extra: ReadonlyArray<string | null | undefined> = [],
): boolean {
  const q = text.trim().toLowerCase();
  if (q === "") return true;
  if (!q.includes(",")) return matchesOne(c, q, extra);
  const parts = q
    .split(",")
    .map((p) => p.trim())
    .filter((p) => p !== "" && p !== "#");
  if (parts.length === 0) return true;
  return parts.some((p) => {
    const bare = p.startsWith("#") ? p.slice(1).trim() : p;
    if (/^\d+$/.test(bare)) return c.id === Number(bare);
    return matchesOne(c, p, extra);
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
      placeholder={searchPlaceholder(["id", "title", "tag", "module", "area", "automation status"])}
      title="Search the Test cases list by case id, title, tag, module, area or automation status. Several words must all match. Separate several searches with commas, for example 135592, 135601"
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
