import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import { SearchInput } from "./search-input";

function Harness({ onClear }: { onClear?: () => void }) {
  const [v, setV] = useState("");
  return (
    <SearchInput
      aria-label="Find things"
      placeholder="Find things"
      value={v}
      onChange={setV}
      onClear={onClear}
      className="w-56"
    />
  );
}

test("the box is 40px tall, text-sm, with a leading icon and the passed-through props", () => {
  render(<Harness />);
  const input = screen.getByLabelText("Find things");
  expect(input).toHaveAttribute("placeholder", "Find things");
  expect(input).toHaveClass("h-10", "text-sm", "w-full");
  // The icon sits inside the same wrapper, decorative.
  const wrapper = input.parentElement!;
  expect(wrapper.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  // className sizes the wrapper, not the field.
  expect(wrapper).toHaveClass("w-56");
});

test("the clear button shows only with text, empties it, and gives the focus back", () => {
  render(<Harness />);
  const input = screen.getByLabelText("Find things") as HTMLInputElement;
  expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();

  fireEvent.change(input, { target: { value: "abc" } });
  expect(input.value).toBe("abc");
  fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
  expect(input.value).toBe("");
  expect(input).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
});

test("a caller's own onClear runs instead of onChange(\"\")", () => {
  const onClear = vi.fn();
  render(<Harness onClear={onClear} />);
  fireEvent.change(screen.getByLabelText("Find things"), { target: { value: "abc" } });
  fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
  expect(onClear).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText("Find things")).toHaveFocus();
});
