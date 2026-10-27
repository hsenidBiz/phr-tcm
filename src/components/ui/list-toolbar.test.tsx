import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ListToolbar, ToolbarSwitch } from "./list-toolbar";

test("the toolbar wraps, the search slot grows and shrinks first, filters never shrink", () => {
  const { container } = render(
    <ListToolbar search={<input aria-label="Find" />}>
      <select aria-label="Outcome" />
    </ListToolbar>,
  );
  expect(container.firstElementChild).toHaveClass("flex", "flex-wrap", "items-center", "gap-2");
  const searchSlot = screen.getByLabelText("Find").parentElement!;
  expect(searchSlot).toHaveClass("flex-1", "min-w-64");
  const filter = screen.getByLabelText("Outcome");
  expect(filter.parentElement).toHaveClass("shrink-0");
  expect(filter.parentElement).not.toBe(searchSlot);
});

test("the toolbar renders with only a search", () => {
  render(<ListToolbar search={<input aria-label="Find" />} />);
  expect(screen.getByLabelText("Find")).toBeInTheDocument();
});

test("the toolbar switch is 40px tall, unboxed, and named by its label", () => {
  render(<ToolbarSwitch checked={false} onCheckedChange={() => {}} label="Group by title" />);
  const sw = screen.getByRole("switch", { name: "Group by title" });
  expect(sw).toHaveAttribute("aria-checked", "false");
  const chip = sw.parentElement!;
  expect(chip).toHaveClass("h-10", "text-sm");
  expect(chip).not.toHaveClass("border");
  expect(chip).not.toHaveClass("rounded-md");
  expect(chip).toHaveTextContent("Group by title");
});

test("clicking the switch or its label toggles", () => {
  const onChange = vi.fn();
  render(<ToolbarSwitch checked={false} onCheckedChange={onChange} label="This sprint" />);
  fireEvent.click(screen.getByRole("switch", { name: "This sprint" }));
  expect(onChange).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByText("This sprint"));
  expect(onChange).toHaveBeenCalledTimes(2);
});

test("a checked switch reports on, and title lands on the chip", () => {
  render(<ToolbarSwitch checked onCheckedChange={() => {}} label="Open lanes only" title="Hide done lanes" />);
  const sw = screen.getByRole("switch", { name: "Open lanes only" });
  expect(sw).toHaveAttribute("aria-checked", "true");
  expect(sw.parentElement).toHaveAttribute("title", "Hide done lanes");
});
