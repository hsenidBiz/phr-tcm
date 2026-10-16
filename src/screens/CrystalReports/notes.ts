// The preview's notes, sorted into what a person can act on.
//
// The renderer writes one line per thing it could not do faithfully
// (`Diagnostic::describe` in the vendored rpt-pages: the message, then
// ` (source)`, then ` [location]`). Two shapes are common enough to say in
// plain words; everything else is kept as it came.
//
// - `formula error: unknown name: {Command.emp_display_number} ...` - the
//   report reads a field the preview's rows do not have (a report with no
//   saved rows, or a live preview whose procedure returned other columns).
// - `font 'Arial' lacks glyph(s) '✓✗' (U+2713 U+2717); ...` - a symbol the
//   report's font has no drawing for, drawn from a fallback font instead.

/** A field the preview had no data for: as the report names it, and as a person reads it. */
export interface MissingField {
  raw: string;
  name: string;
}

export interface PreviewNotes {
  /** Distinct fields, in the order they were first reported. */
  fields: MissingField[];
  /** Distinct symbols drawn from a fallback font, in the order first reported. */
  glyphs: string[];
  /** Every other note, exactly as it came. */
  other: string[];
}

// A database field is `{Table.field}` (the table part may itself hold
// dots). The evaluator writes an unknown formula or parameter without its
// sigil (`{Year}`, not `{@Year}` or `{?Year}`), so it has no dot and is not
// a field; an unknown function has no braces at all. Both stay in the
// other notes.
const FIELD = /^formula error: unknown name: (\{[^{}]*\.[^{}]*\})/;
const GLYPHS = /^font '.*?' lacks glyph\(s\) '(.+?)' \(U\+/;

/** `{Command.emp_display_number}` as "Emp display number". */
export function friendlyFieldName(raw: string): string {
  const inner = raw.replace(/^\{|\}$/g, "");
  const words = inner
    .slice(inner.lastIndexOf(".") + 1)
    .replace(/_+/g, " ")
    .trim();
  if (!words) return raw;
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function classifyNotes(lines: readonly string[]): PreviewNotes {
  const fields: MissingField[] = [];
  const glyphs: string[] = [];
  const other: string[] = [];
  for (const line of lines) {
    const field = FIELD.exec(line);
    if (field) {
      const raw = field[1];
      if (!fields.some((f) => f.raw === raw)) fields.push({ raw, name: friendlyFieldName(raw) });
      continue;
    }
    const glyph = GLYPHS.exec(line);
    if (glyph) {
      for (const g of Array.from(glyph[1])) if (!glyphs.includes(g)) glyphs.push(g);
      continue;
    }
    other.push(line);
  }
  return { fields, glyphs, other };
}

export function missingFieldsHeadline(count: number): string {
  return count === 1 ? "1 field has no data in this preview." : `${count} fields have no data in this preview.`;
}

/** Why the fields are empty, by where the preview's rows came from (`live` is the connection's name). */
export function missingFieldsBody(live: string | null): string {
  return live
    ? "Your live preview's results did not include these fields."
    : "This report holds no saved rows. Ask your AI assistant for a live preview on the Dev login to fill them.";
}

export function symbolsNote(glyphs: readonly string[]): string {
  const count = glyphs.length === 1 ? "1 symbol" : `${glyphs.length} symbols`;
  return `${count} drawn with a fallback font (${glyphs.join(" ")})`;
}
