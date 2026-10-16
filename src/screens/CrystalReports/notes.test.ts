// The preview's notes as a person reads them: which fields had no data,
// which symbols were drawn from a fallback font, and everything else as it
// came. The strings below are the exact shapes the renderer writes
// (`Diagnostic::describe`: message, then ` (source)`, then ` [location]`).

import { describe, expect, test } from "vitest";
import { classifyNotes, friendlyFieldName, missingFieldsBody, missingFieldsHeadline, symbolsNote } from "./notes";

const FIELD = (raw: string, tail = " (Field12) [Details, bytes 0..32]") => `formula error: unknown name: ${raw}${tail}`;
const GLYPHS = (font: string, glyphs: string, codes: string) =>
  `font '${font}' lacks glyph(s) '${glyphs}' (${codes}); rendered from a fallback face`;

describe("friendlyFieldName", () => {
  test.each([
    ["{Command.emp_display_number}", "Emp display number"],
    ["{GoalAlign.goal}", "Goal"],
    ["{dbo.Employees.first__name}", "First name"],
    ["{Command.EMP_NAME}", "EMP NAME"],
    ["{Command._status_}", "Status"],
    ["{Command.}", "{Command.}"],
  ])("%s reads as %s", (raw, name) => {
    expect(friendlyFieldName(raw)).toBe(name);
  });
});

describe("classifyNotes", () => {
  test("an unknown field name is a missing field, counted once however often it failed", () => {
    const notes = classifyNotes([
      FIELD("{Command.emp_display_number}"),
      FIELD("{Command.emp_display_number}", " (Field13) [Group Header #1, bytes 2..30]"),
      FIELD("{Command.goal_name}", ""),
    ]);
    expect(notes.fields).toEqual([
      { raw: "{Command.emp_display_number}", name: "Emp display number" },
      { raw: "{Command.goal_name}", name: "Goal name" },
    ]);
    expect(notes.glyphs).toEqual([]);
    expect(notes.other).toEqual([]);
  });

  test("a missing glyph lists each distinct symbol once, across fonts", () => {
    const notes = classifyNotes([
      GLYPHS("Arial", "✓✗", "U+2713 U+2717"),
      GLYPHS("Tahoma", "✓", "U+2713"),
      GLYPHS("Arial", "★", "U+2605"),
    ]);
    expect(notes.glyphs).toEqual(["✓", "✗", "★"]);
    expect(notes.fields).toEqual([]);
    expect(notes.other).toEqual([]);
  });

  test("anything else is kept as it came, in order", () => {
    const other = [
      "formula error: unknown name: NoSuchFn (Formula @Total)",
      // An unknown formula or parameter name is written without its @ or ?
      // sigil - no dot, so it is not a field.
      "formula error: unknown name: {Year} (Field1)",
      "formula error: divide by zero (Formula @Ratio)",
      "unsupported in formula: CrossTab",
      "The preview of Goals.rpt could not be drawn: no pages.",
    ];
    const notes = classifyNotes([other[0], FIELD("{Command.a}"), ...other.slice(1)]);
    expect(notes.other).toEqual(other);
    expect(notes.fields.map((f) => f.raw)).toEqual(["{Command.a}"]);
  });

  test("no notes is nothing in any group", () => {
    expect(classifyNotes([])).toEqual({ fields: [], glyphs: [], other: [] });
  });
});

describe("wording", () => {
  test("the headline counts the fields, singular for one", () => {
    expect(missingFieldsHeadline(1)).toBe("1 field has no data in this preview.");
    expect(missingFieldsHeadline(4)).toBe("4 fields have no data in this preview.");
  });

  test("the body says why, by where the data came from", () => {
    expect(missingFieldsBody(null)).toBe(
      "This report holds no saved rows. Ask your AI assistant for a live preview on the Dev login to fill them.",
    );
    expect(missingFieldsBody("Reports dev")).toBe("Your live preview's results did not include these fields.");
  });

  test("the symbols note counts and shows them, singular for one", () => {
    expect(symbolsNote(["✓"])).toBe("1 symbol drawn with a fallback font (✓)");
    expect(symbolsNote(["✓", "✗"])).toBe("2 symbols drawn with a fallback font (✓ ✗)");
  });
});
