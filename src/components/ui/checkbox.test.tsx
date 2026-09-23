// The mixed state: "some but not all" on group-header checkboxes.

import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Checkbox } from "./checkbox";

test("indeterminate reads as mixed, and a click completes the selection", () => {
  const onChange = vi.fn();
  render(
    <Checkbox
      checked={false}
      indeterminate
      onCheckedChange={onChange}
      ariaLabel="Select all in Login"
    />,
  );
  const box = screen.getByRole("checkbox", { name: "Select all in Login" });
  expect(box).toHaveAttribute("aria-checked", "mixed");
  // From mixed, clicking selects the REST - never deselects the part.
  fireEvent.click(box);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("checked wins over a stale indeterminate flag", () => {
  render(<Checkbox checked indeterminate onCheckedChange={() => {}} ariaLabel="All" />);
  expect(screen.getByRole("checkbox", { name: "All" })).toHaveAttribute("aria-checked", "true");
});

// A wrapping <label> is how most boxes in the app get their name
// (PowerRenameDialog, multiselect, BulkEditDialog): it must name the box,
// and one click - on the words or on the box - must toggle it once.
test("a wrapping label names the box, and clicking its text toggles once", () => {
  const onChange = vi.fn();
  render(
    <label>
      <Checkbox checked={false} onCheckedChange={onChange} />
      Apply module
    </label>,
  );
  expect(screen.getByRole("checkbox", { name: "Apply module" })).toHaveAttribute("data-slot", "checkbox");
  fireEvent.click(screen.getByText("Apply module"));
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("clicking the box itself inside a label also toggles once", () => {
  const onChange = vi.fn();
  render(
    <label>
      <Checkbox checked={false} onCheckedChange={onChange} />
      Match case
    </label>,
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "Match case" }));
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(true);
});

test("Space toggles it", () => {
  const onChange = vi.fn();
  render(<Checkbox checked={false} onCheckedChange={onChange} ariaLabel="Pick" />);
  const box = screen.getByRole("checkbox", { name: "Pick" });
  fireEvent.keyDown(box, { key: " " });
  fireEvent.keyUp(box, { key: " " });
  expect(onChange).toHaveBeenCalledWith(true);
});

// QueueRow puts a box inside a clickable row and relies on the row hearing
// the click - once.
test("a click on the box still reaches the row it sits in, once", () => {
  const row = vi.fn();
  const onChange = vi.fn();
  render(
    <div onClick={row}>
      <Checkbox checked onCheckedChange={onChange} ariaLabel="Select Login works" />
    </div>,
  );
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Login works" }));
  expect(row).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(false);
});

// I-1: Base UI's own wrapping-<label> fallback only applies when nothing
// else already names the box. An explicit ariaLabel must win over it -
// otherwise a call site whose visible label text differs from its
// ariaLabel gets a wrong (or, per Base UI's embedded-control substitution,
// doubled) accessible name.
test("an explicit ariaLabel wins over the wrapping label's own words", () => {
  const onChange = vi.fn();
  render(
    <label>
      <Checkbox ariaLabel="Select all queued cases" checked={false} onCheckedChange={onChange} />
      Select cases for bulk actions
    </label>,
  );
  const box = screen.getByRole("checkbox", { name: "Select all queued cases" });
  expect(screen.queryByRole("checkbox", { name: /Select cases for bulk actions/ })).toBeNull();
  // Still one toggle for a click on the words, same as with no ariaLabel at all.
  fireEvent.click(screen.getByText("Select cases for bulk actions"));
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(onChange).toHaveBeenCalledWith(true);
  expect(box).toBeInTheDocument();
});

test("with no ariaLabel, a wrapping label still names the box", () => {
  render(
    <label>
      <Checkbox checked={false} onCheckedChange={() => {}} />
      Group by title
    </label>,
  );
  expect(screen.getByRole("checkbox", { name: "Group by title" })).toBeInTheDocument();
});
