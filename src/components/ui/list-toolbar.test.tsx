import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ListToolbar, ToolbarSwitch } from "./list-toolbar";

test("the toolbar wraps, the search slot grows and shrinks first, filters never shrink", () => {
  const { container } = render(
    <ListToolbar search={<input aria-label="Find" />}>
      <select aria-label="Outcome" />
    </ListToolbar>,
  );
  const row = container.firstElementChild!;
  expect(row).toHaveClass("flex", "flex-wrap", "items-center", "gap-2", "*:shrink-0");
  const searchSlot = screen.getByLabelText("Find").parentElement!;
  expect(searchSlot).toHaveClass("flex-1", "min-w-64");
  // A filter sits straight in the row (the row's *:shrink-0 holds it).
  expect(screen.getByLabelText("Outcome").parentElement).toBe(row);
});

test("a filter that renders nothing leaves no empty box, so the gaps stay even", () => {
  const Nothing = () => null;
  const scope = "";
  const { container } = render(
    <ListToolbar search={<input aria-label="Find" />}>
      <Nothing />
      {scope && <span>never</span>}
      <select aria-label="Outcome" />
    </ListToolbar>,
  );
  // The search slot and the one real filter: nothing in between.
  expect(container.firstElementChild!.children).toHaveLength(2);
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
