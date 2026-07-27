// The markdown toolbar's actual work: given the text and the selection,
// return the new text and where the caret should end up.
//
// Kept pure and away from the DOM so the fiddly parts - toggling a wrap
// off when it's already there, prefixing every line of a multi-line
// selection, leaving the caret somewhere useful - are testable.

export type MdAction =
  | "bold"
  | "italic"
  | "code"
  | "link"
  | "bullet"
  | "number"
  | "heading"
  | "quote";

export type MdEdit = { value: string; start: number; end: number };

/** Wrappers toggle: applying bold to already-bold text unbolds it. */
const WRAP: Partial<Record<MdAction, string>> = {
  bold: "**",
  italic: "_",
  code: "`",
};

/** Line prefixes. `number` is special-cased - it counts. */
const PREFIX: Partial<Record<MdAction, string>> = {
  bullet: "- ",
  heading: "## ",
  quote: "> ",
};

/** Expand a selection to whole lines, so line-prefix actions apply to
 * every line the user touched rather than a fragment of the first. */
function lineSpan(value: string, start: number, end: number): [number, number] {
  const from = value.lastIndexOf("\n", start - 1) + 1;
  const nextBreak = value.indexOf("\n", end);
  return [from, nextBreak === -1 ? value.length : nextBreak];
}

function applyPrefix(value: string, start: number, end: number, action: MdAction): MdEdit {
  const [from, to] = lineSpan(value, start, end);
  const lines = value.slice(from, to).split("\n");
  const numbered = action === "number";
  const prefixOf = (i: number) => (numbered ? `${i + 1}. ` : PREFIX[action]!);

  // Already prefixed everywhere -> remove it, so the button toggles.
  const has = lines.every((l, i) =>
    numbered ? /^\d+\.\s/.test(l) : l.startsWith(prefixOf(i)),
  );
  const next = lines
    .map((l, i) =>
      has
        ? numbered
          ? l.replace(/^\d+\.\s/, "")
          : l.slice(prefixOf(i).length)
        : prefixOf(i) + l,
    )
    .join("\n");

  return {
    value: value.slice(0, from) + next + value.slice(to),
    start: from,
    end: from + next.length,
  };
}

/**
 * Apply a toolbar action to `value` over the selection [start, end).
 * With an empty selection, wrap actions insert their markers and put the
 * caret between them, so typing continues inside the formatting.
 */
export function applyMd(value: string, start: number, end: number, action: MdAction): MdEdit {
  if (action === "link") {
    const text = value.slice(start, end);
    const inserted = `[${text || "text"}](url)`;
    // Caret on "url" - the part that always needs replacing.
    const urlAt = start + inserted.length - 4;
    return {
      value: value.slice(0, start) + inserted + value.slice(end),
      start: urlAt,
      end: urlAt + 3,
    };
  }

  const wrap = WRAP[action];
  if (!wrap) return applyPrefix(value, start, end, action);

  const selected = value.slice(start, end);
  const before = value.slice(0, start);
  const after = value.slice(end);

  // Already wrapped (either inside the selection or just outside it) ->
  // unwrap, so the button toggles rather than stacking `****bold****`.
  if (selected.startsWith(wrap) && selected.endsWith(wrap) && selected.length >= wrap.length * 2) {
    const inner = selected.slice(wrap.length, selected.length - wrap.length);
    return { value: before + inner + after, start, end: start + inner.length };
  }
  if (before.endsWith(wrap) && after.startsWith(wrap)) {
    return {
      value: before.slice(0, -wrap.length) + selected + after.slice(wrap.length),
      start: start - wrap.length,
      end: end - wrap.length,
    };
  }

  const inserted = wrap + selected + wrap;
  return {
    value: before + inserted + after,
    start: start + wrap.length,
    end: start + wrap.length + selected.length,
  };
}
