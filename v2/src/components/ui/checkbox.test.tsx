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
