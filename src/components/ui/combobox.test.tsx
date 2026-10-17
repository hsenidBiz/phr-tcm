// Combobox: free entry, and the keyboard hand-offs a form built on it needs.

import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Combobox from "./combobox";

function mount(onChange = vi.fn(), onKeyDown = vi.fn()) {
  render(
    <div onKeyDown={(e) => onKeyDown(e.key)}>
      <Combobox ariaLabel="Folder" value="" onChange={onChange} options={["a/one", "a/two"]} allowCustom />
    </div>,
  );
  return { onChange, onKeyDown };
}

test("a value that is not in the list can be entered", () => {
  const { onChange } = mount();
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: "  b/new  " } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("b/new");
});

test("Escape in the open list closes only the list and keeps the keyboard on the field", () => {
  const { onKeyDown } = mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByPlaceholderText("Search…"), { key: "Escape" });
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  // Whatever holds the combobox (a form, a dialog) is not told: it was the list that answered.
  expect(onKeyDown).not.toHaveBeenCalled();
});

test("Escape on the closed field is left for whatever holds it", () => {
  const { onKeyDown } = mount();
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Folder" }), { key: "Escape" });
  expect(onKeyDown).toHaveBeenCalledWith("Escape");
});

test("choosing with Enter hands the keyboard back to the field", () => {
  const { onChange } = mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByPlaceholderText("Search…"), { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("a/one");
  expect(trigger).toHaveFocus();
});

test("choosing with the mouse does not move focus", () => {
  mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("option", { name: "a/two" }));
  expect(trigger).not.toHaveFocus();
});

test("a typed value that is only the start of an option is kept, not swapped for the option", () => {
  const { onChange } = mount();
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: "a" } });
  // The typed value leads the list and is what Enter takes.
  const rows = Array.from(screen.getByRole("listbox").querySelectorAll("li"));
  expect(rows[0]).toHaveTextContent('Use "a"');
  expect(rows).toHaveLength(3);
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("a");
});

test("a typed value that is an option, whatever its case, takes that option", () => {
  const { onChange } = mount();
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: " A/TWO " } });
  expect(screen.queryByText(/^Use "/)).not.toBeInTheDocument();
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("a/two");
});

test("the arrow keys step from the typed value into the matching options", () => {
  const { onChange } = mount();
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: "a/" } });
  fireEvent.keyDown(box, { key: "ArrowDown" });
  fireEvent.keyDown(box, { key: "ArrowDown" });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("a/two");
});

test("seedQuery opens the search box with the current value, to be extended by typing", () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <Combobox ariaLabel="Folder" value="a/b" onChange={onChange} options={["a/b/c", "a/b"]} allowCustom seedQuery />,
  );
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  expect(box).toHaveValue("a/b");
  // Enter on the seeded value keeps it, even though another option starts with it.
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenLastCalledWith("a/b");
  // Without the prop the box starts empty, as it always did.
  rerender(<Combobox ariaLabel="Folder" value="a/b" onChange={onChange} options={["a/b/c", "a/b"]} allowCustom />);
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  expect(screen.getByPlaceholderText("Search…")).toHaveValue("");
});
