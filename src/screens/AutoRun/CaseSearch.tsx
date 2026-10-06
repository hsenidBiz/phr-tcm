// The search box above the Auto Run case list. It narrows the list by the
// case id (with or without its "#") or by words in the title, together with
// the result filters. Escape empties it while it has focus; so does the
// clear button beside it.

import { Input } from "../../components/ui/input";
import { IconClear } from "../../lib/actionIcons";

/** Whether a case matches what was typed. Blank text matches every case.
 * Letter case is ignored, and "#12" finds the same ids "12" does: every
 * id with 12 in it. */
export function matchesSearch(c: { id: number; title: string }, text: string): boolean {
  const q = text.trim().toLowerCase();
  if (q === "") return true;
  const bare = q.startsWith("#") ? q.slice(1).trim() : q;
  if (bare !== "" && String(c.id).includes(bare)) return true;
  return c.title.toLowerCase().includes(q);
}

export default function CaseSearch({ value, onChange }: { value: string; onChange: (text: string) => void }) {
  return (
    <span className="relative inline-flex items-center">
      <Input
        aria-label="Search test cases"
        className="w-56 py-1.5 pl-2 pr-8"
        placeholder="Search by id or title"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value !== "") {
            // Only this box's own text: a dialog behind it is not closed.
            e.stopPropagation();
            onChange("");
          }
        }}
      />
      {value !== "" && (
        <button
          type="button"
          aria-label="Clear search"
          title="Clear search"
          className="absolute right-1.5 rounded p-1 text-muted transition-colors hover:text-accent [&_svg]:size-3.5"
          onClick={() => onChange("")}
        >
          <IconClear aria-hidden />
        </button>
      )}
    </span>
  );
}
