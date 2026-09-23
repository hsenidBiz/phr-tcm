// Textarea is XiodUI's: a styled box (data-slot="textarea-control") with the
// <textarea> filling it. Call sites size the BOX - h-40, w-full, flex-1,
// min-h-... - exactly as they sized the old bare textarea, and never change.

import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { expect, test, vi } from "vitest";
import { Textarea } from "./input";

const box = (name: string) => screen.getByRole("textbox", { name }).parentElement!;

test("the call site's classes size the box; the field inside fills it", () => {
  render(<Textarea aria-label="Notes" className="h-40 w-full font-mono" />);
  const field = screen.getByRole("textbox", { name: "Notes" });
  expect(field.tagName).toBe("TEXTAREA");
  expect(field).toHaveAttribute("data-slot", "textarea");
  expect(box("Notes")).toHaveAttribute("data-slot", "textarea-control");
  expect(box("Notes")).toHaveClass("h-40", "w-full", "font-mono");
  expect(box("Notes")).toHaveClass("[&>textarea]:min-h-0", "[&>textarea]:[field-sizing:fixed]");
});

// I-3: a percentage height (h-full) against a box whose own height comes
// from min-h-*/flex-1 - an indefinite height - resolves to auto, so it
// cannot fill it; only the flex box's own default stretch can. A box sized
// by min-h-*/flex-1 alone (no h-*) must never carry that class on the
// field, in any window size.
test("a box sized by min-h/flex-1 alone relies on the flex box's own stretch, not a height on the field", () => {
  render(<Textarea aria-label="Notes" className="min-h-40 flex-1" />);
  expect(box("Notes")).toHaveClass("min-h-40", "flex-1");
  expect(box("Notes")).not.toHaveClass("[&>textarea]:h-full");
});

test("the box is what the user drags taller", () => {
  render(<Textarea aria-label="Notes" />);
  expect(box("Notes")).toHaveClass("resize-y", "overflow-hidden", "[&>textarea]:resize-none");
});

// XiodUI sets `sm:text-sm` on the box, and a breakpoint class outranks a
// plain one - so without its twin, a call site's text-xs would lose in any
// window wider than 640px (BugDialog's repro steps, RunPane, RecipeEditor).
test("a call site's text size survives XiodUI's own size at sm: and up", () => {
  render(<Textarea aria-label="Repro steps" className="h-40 w-full font-mono text-xs" />);
  expect(box("Repro steps")).toHaveClass("text-xs", "sm:text-xs");
  expect(box("Repro steps")).not.toHaveClass("sm:text-sm");
  expect(box("Repro steps")).not.toHaveClass("text-base");
});

test("with no size given it reads at text-sm, like an Input beside it", () => {
  render(<Textarea aria-label="Notes" />);
  expect(box("Notes")).toHaveClass("text-sm", "sm:text-sm");
  expect(box("Notes")).not.toHaveClass("text-base");
});

// MarkdownField edits the selection through this ref.
test("the forwarded ref is the <textarea> itself", () => {
  const ref = createRef<HTMLTextAreaElement>();
  render(<Textarea ref={ref} aria-label="Notes" defaultValue="hello" />);
  expect(ref.current).toBe(screen.getByRole("textbox", { name: "Notes" }));
  expect(ref.current?.value).toBe("hello");
});

test("value and onChange reach the field", () => {
  const onChange = vi.fn();
  render(<Textarea aria-label="Notes" value="a" onChange={onChange} />);
  const field = screen.getByRole("textbox", { name: "Notes" });
  expect(field).toHaveValue("a");
  fireEvent.change(field, { target: { value: "ab" } });
  expect(onChange).toHaveBeenCalledTimes(1);
});

test("disabled reaches the field", () => {
  render(<Textarea aria-label="Notes" disabled />);
  expect(screen.getByRole("textbox", { name: "Notes" })).toBeDisabled();
});
